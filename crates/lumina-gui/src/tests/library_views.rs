//! library filter, views and G-09 file operations tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

#[test]
fn w3_library_filter_matches_names_and_metadata() {
    // Empty query matches everything (default grid is unfiltered).
    assert!(library_filter_matches(
        "IMG_0001.ARW",
        0,
        Flag::Unflagged,
        0,
        ""
    ));
    assert!(library_filter_matches(
        "IMG_0001.ARW",
        4,
        Flag::Pick,
        1,
        "   "
    ));
    // Plain text: case-insensitive substring on the file name.
    assert!(library_filter_matches(
        "IMG_0001.ARW",
        0,
        Flag::Unflagged,
        0,
        "img_0001"
    ));
    assert!(!library_filter_matches(
        "IMG_0001.ARW",
        0,
        Flag::Unflagged,
        0,
        "cr2"
    ));
    // Structured rating filter.
    assert!(library_filter_matches(
        "a.arw",
        4,
        Flag::Unflagged,
        0,
        "rating:4"
    ));
    assert!(!library_filter_matches(
        "a.arw",
        4,
        Flag::Unflagged,
        0,
        "rating:5"
    ));
    // Recognised prefix with an unparseable value matches nothing
    // (visible empty grid, never a silent pass-through).
    assert!(!library_filter_matches(
        "rating:4.arw",
        4,
        Flag::Unflagged,
        0,
        "rating:x"
    ));
    // Structured flag filter.
    assert!(library_filter_matches(
        "a.arw",
        0,
        Flag::Pick,
        0,
        "flag:pick"
    ));
    assert!(!library_filter_matches(
        "a.arw",
        0,
        Flag::Pick,
        0,
        "flag:reject"
    ));
    assert!(!library_filter_matches(
        "a.arw",
        0,
        Flag::Pick,
        0,
        "flag:bogus"
    ));
    // Structured color-label filter.
    assert!(library_filter_matches(
        "a.arw",
        0,
        Flag::Unflagged,
        1,
        "label:red"
    ));
    assert!(library_filter_matches(
        "a.arw",
        0,
        Flag::Unflagged,
        0,
        "label:none"
    ));
    assert!(!library_filter_matches(
        "a.arw",
        0,
        Flag::Unflagged,
        1,
        "label:blue"
    ));
    assert!(!library_filter_matches(
        "a.arw",
        0,
        Flag::Unflagged,
        1,
        "label:bogus"
    ));
}

#[test]
fn w3_filter_bar_toggle_is_display_only() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let generation = app.preview_generation();
    assert!(!app.filter_bar_visible);
    app.toggle_filter_bar();
    assert!(app.filter_bar_visible);
    app.set_library_filter("img");
    assert_eq!(app.library_filter, "img");
    app.toggle_filter_bar();
    assert!(!app.filter_bar_visible);
    // View state only: recipe and render generation are untouched.
    assert!(app.recipe().adjustments.is_empty());
    assert_eq!(app.preview_generation(), generation);
}

#[test]
fn w3_compare_toggle_reuses_before_after() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let generation = app.preview_generation();
    assert_eq!(app.compare_mode(), None);
    assert!(!app.before_after);
    app.toggle_compare_mode(CompareMode::Compare);
    assert_eq!(app.compare_mode(), Some(CompareMode::Compare));
    assert!(app.before_after);
    // Display-only: the recipe (and therefore any sidecar state) and the
    // render generation are untouched.
    assert!(app.recipe().adjustments.is_empty());
    assert_eq!(app.preview_generation(), generation);
    app.toggle_compare_mode(CompareMode::Compare);
    assert_eq!(app.compare_mode(), None);
    assert!(!app.before_after);
}

#[test]
fn w3_survey_toggle_jumps_to_library_grid() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.set_module(Module::Develop);
    app.toggle_compare_mode(CompareMode::Survey);
    assert_eq!(app.compare_mode(), Some(CompareMode::Survey));
    assert_eq!(app.active_module, Module::Library);
    assert!(!app.before_after);
    assert!(app.recipe().adjustments.is_empty());
    // A repeat press leaves survey mode but stays on the grid (no forced
    // module return).
    app.toggle_compare_mode(CompareMode::Survey);
    assert_eq!(app.compare_mode(), None);
    assert_eq!(app.active_module, Module::Library);
}

// ---- G-09 Library parity (LRPAR-G09-LIB): views, navigation, folders ----

#[test]
fn g09_library_view_for_key_maps_g_e_c_n() {
    assert_eq!(library_view_for_key(egui::Key::G), Some(LibraryView::Grid));
    assert_eq!(library_view_for_key(egui::Key::E), Some(LibraryView::Loupe));
    assert_eq!(
        library_view_for_key(egui::Key::C),
        Some(LibraryView::Compare)
    );
    assert_eq!(
        library_view_for_key(egui::Key::N),
        Some(LibraryView::Survey)
    );
    assert_eq!(library_view_for_key(egui::Key::Y), None);
    assert_eq!(library_view_for_key(egui::Key::D), None);
    assert_eq!(library_view_for_key(egui::Key::V), None);
}

#[test]
fn g09_library_move_index_clamps_without_wrap() {
    assert_eq!(library_move_index(0, 0, 0), 0);
    assert_eq!(library_move_index(0, 1, 3), 1);
    assert_eq!(library_move_index(2, 1, 3), 2);
    assert_eq!(library_move_index(0, -1, 3), 0);
    assert_eq!(library_move_index(1, -5, 3), 0);
    assert_eq!(library_move_index(1, 99, 3), 2);
    assert_eq!(library_move_index(9, 1, 3), 2);
    assert_eq!(library_move_index(0, isize::MIN / 2, 3), 0);
    assert_eq!(library_move_index(0, isize::MAX / 2, 3), 2);
}

#[test]
fn g09_set_library_view_syncs_module_and_before_after() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    let generation = app.preview_generation();
    assert_eq!(app.library_view(), LibraryView::Grid);
    app.set_library_view(LibraryView::Loupe);
    assert_eq!(app.library_view(), LibraryView::Loupe);
    assert_eq!(app.module(), Module::Library);
    assert!(!app.before_after);
    app.set_library_view(LibraryView::Compare);
    assert_eq!(app.library_view(), LibraryView::Compare);
    assert_eq!(app.compare_mode(), Some(CompareMode::Compare));
    assert!(app.before_after);
    app.set_library_view(LibraryView::Survey);
    assert_eq!(app.library_view(), LibraryView::Survey);
    assert_eq!(app.compare_mode(), Some(CompareMode::Survey));
    assert!(!app.before_after);
    app.set_library_view(LibraryView::Grid);
    assert_eq!(app.library_view(), LibraryView::Grid);
    assert_eq!(app.compare_mode(), None);
    // Display-only: recipe and render generation are untouched.
    assert!(app.recipe().adjustments.is_empty());
    assert_eq!(app.preview_generation(), generation);
}

#[test]
fn g09_compare_toggle_syncs_library_view() {
    let mut app = new_app();
    app.load_bytes(png(), "test.png").unwrap();
    app.toggle_compare_mode(CompareMode::Compare);
    assert_eq!(app.library_view(), LibraryView::Grid);
    // The G-09 key path (update loop) syncs the view after the toggle;
    // mirror that here: Compare proxy -> Compare view, repeat -> Grid.
    app.set_library_view(LibraryView::Compare);
    assert!(app.before_after);
    app.toggle_compare_mode(CompareMode::Survey);
    app.set_library_view(LibraryView::Survey);
    assert_eq!(app.module(), Module::Library);
    assert!(!app.before_after);
}

#[test]
fn g09_move_library_selection_walks_filtered_raster() {
    let root = tempfile::tempdir().unwrap();
    save_raw(&root.path().join("a.arw"));
    save_raw(&root.path().join("b.arw"));
    save_raw(&root.path().join("c.arw"));
    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    app.list_directory();
    assert_eq!(app.filtered_library_order().len(), 3);
    let first = app.entries()[app.filtered_library_order()[0]]
        .path
        .display()
        .to_string();
    app.select_filmstrip_path(first.clone(), false, false);
    app.path = first.clone();
    let second = app.move_library_selection(1).unwrap();
    assert_ne!(second, first);
    assert_eq!(app.filmstrip_selection(), vec![second.clone()]);
    // Clamp at the end: moving past the last entry stays.
    app.path = second.clone();
    let last = app.move_library_selection(99).unwrap();
    app.path = last.clone();
    assert_eq!(app.move_library_selection(1).unwrap(), last);
    // Home clamps to the first entry.
    assert_eq!(app.move_library_selection(isize::MIN / 2).unwrap(), first);
    // The shared filter narrows navigation and painting identically.
    app.set_library_filter("b.arw");
    assert_eq!(app.filtered_library_order().len(), 1);
}

#[test]
fn g09_open_library_selection_loads_and_shows_loupe() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    let path = source.display().to_string();
    app.filmstrip_selection = BTreeSet::from([path.clone()]);
    app.open_library_selection();
    assert_eq!(app.library_view(), LibraryView::Loupe);
    for _ in 0..2000 {
        app.poll_decode();
        if app.original.is_some() || app.error().is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(app.original.is_some(), "loupe opens the selected file");
}

/// G-09 (B2): opening the selection routes through `set_library_view`,
/// so a stale Compare proxy (`compare_mode`/`before_after`) is cleared.
#[test]
fn g09_open_library_selection_clears_stale_compare_proxy() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    let path = source.display().to_string();
    app.filmstrip_selection = BTreeSet::from([path.clone()]);
    app.set_library_view(LibraryView::Compare);
    assert!(app.before_after);
    app.open_library_selection();
    assert_eq!(app.library_view(), LibraryView::Loupe);
    assert_eq!(app.compare_mode(), None);
    assert!(!app.before_after);
    assert!(app.recipe().adjustments.is_empty());
}

#[test]
fn g09_create_and_rename_folder_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    let fresh = root.path().join("fresh");
    app.create_folder(&fresh).unwrap();
    assert!(fresh.is_dir());
    // Creating over an existing directory is idempotent, not an error.
    app.create_folder(&fresh).unwrap();
    // A file at the target path is a loud error, never a silent clobber.
    let blocker = root.path().join("blocker.png");
    save_png(&blocker);
    assert!(app.create_folder(&blocker).is_err());
    let renamed = root.path().join("renamed");
    app.rename_folder(&fresh, &renamed).unwrap();
    assert!(renamed.is_dir());
    assert!(!fresh.exists());
    // Renaming onto an existing target is loudly refused.
    assert!(app.rename_folder(&renamed, root.path()).is_err());
    assert!(app
        .rename_folder(&root.path().join("missing"), &fresh)
        .is_err());
}

#[test]
fn g09_move_image_carries_sidecars_and_reloads() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    open_and_decode(&mut app, source.display().to_string());
    app.set_adjustment("exposure", 1.5);
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&source);
    assert!(sidecar.is_file(), "sidecar must exist before the move");
    let dest_dir = root.path().join("album");
    app.create_folder(&dest_dir).unwrap();
    let target = app.move_image_to_folder(&source, &dest_dir).unwrap();
    assert!(!source.exists());
    assert!(target.is_file());
    let moved_sidecar = lumina_sidecar::sidecar_path_for(&target);
    assert!(moved_sidecar.is_file(), "sidecar must follow the image");
    assert!(!sidecar.exists());
    // Roundtrip: the moved sidecar still validates and carries the edit.
    let document = lumina_sidecar::load_sidecar(&moved_sidecar).unwrap();
    assert_eq!(
        document.virtual_copies[0].recipe.adjustments["exposure"],
        1.5
    );
    // The moved image reloads with its recipe (Edit -> Commit -> Datei
    // -> Reload over a folder move).
    let mut reopened = new_app();
    open_and_decode(&mut reopened, target.display().to_string());
    assert_eq!(reopened.recipe().adjustments["exposure"], 1.5);
    // No absolute paths leaked into the persisted sidecar.
    let raw = std::fs::read_to_string(&moved_sidecar).unwrap();
    assert!(
        !raw.contains(&root.path().display().to_string()),
        "sidecar must not persist absolute paths"
    );
}

#[test]
fn g09_move_image_refuses_existing_target() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("photo.png");
    save_png(&source);
    let dest_dir = root.path().join("album");
    std::fs::create_dir(&dest_dir).unwrap();
    save_png(&dest_dir.join("photo.png"));
    let mut app = new_app();
    let before = std::fs::read(&source).unwrap();
    assert!(app.move_image_to_folder(&source, &dest_dir).is_err());
    assert!(source.is_file(), "refused move must keep the source");
    assert_eq!(std::fs::read(&source).unwrap(), before);
}

#[test]
fn g09_delete_image_removes_companions_and_keeps_selection_sane() {
    let root = tempfile::tempdir().unwrap();
    let gone = root.path().join("gone.png");
    let kept = root.path().join("kept.png");
    save_png(&gone);
    save_png(&kept);
    let mut app = new_app();
    app.set_directory(root.path().display().to_string());
    open_and_decode(&mut app, gone.display().to_string());
    app.save_sidecar();
    let sidecar = lumina_sidecar::sidecar_path_for(&gone);
    assert!(sidecar.is_file());
    app.delete_image_with_sidecars(&gone).unwrap();
    assert!(!gone.exists());
    assert!(
        app.path.trim().is_empty(),
        "deleting the loaded image clears the session path (no orphan save)"
    );
    assert!(app.sidecar_revision.is_none());
    assert!(!sidecar.exists(), "companions must go with the image");
    assert!(kept.is_file(), "siblings must survive");
    assert!(app.delete_image_with_sidecars(&gone).is_err());
}

/// REVIEW-GUI-MOVE-1: the discriminating negative for the delete cleanup.
/// The image is loaded **without** a sidecar (`sidecar_revision == None`),
/// so if the session path were not cleared, the follow-up save would use
/// `expected = None` and create an orphan sidecar at the deleted location.
#[test]
fn g09_delete_unsaved_image_cannot_recreate_orphan_sidecar() {
    let root = tempfile::tempdir().unwrap();
    let photo = root.path().join("photo.png");
    save_png(&photo);
    let sidecar = lumina_sidecar::sidecar_path_for(&photo);
    let mut app = new_app();
    open_and_decode(&mut app, photo.display().to_string());
    assert!(
        app.sidecar_revision.is_none(),
        "no sidecar exists before the delete"
    );
    assert!(!sidecar.exists());
    app.delete_image_with_sidecars(&photo).unwrap();
    assert!(app.path.trim().is_empty(), "session path cleared");
    app.save_sidecar();
    assert!(
        !sidecar.exists(),
        "a save after delete must not recreate an orphan sidecar"
    );
}

/// REVIEW-GUI-MOVE-1: moving the loaded image re-points `self.path` (and
/// its CAS revision) at the moved bundle so the next save cannot orphan a
/// sidecar at the old location.
#[test]
fn g09_move_image_repoints_loaded_session() {
    let root = tempfile::tempdir().unwrap();
    let src_dir = root.path().join("src");
    let dst_dir = root.path().join("dst");
    std::fs::create_dir(&src_dir).unwrap();
    std::fs::create_dir(&dst_dir).unwrap();
    let source = src_dir.join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.save_sidecar();
    assert!(lumina_sidecar::sidecar_path_for(&source).is_file());
    let target = app.move_image_to_folder(&source, &dst_dir).unwrap();
    assert!(!source.exists());
    assert_eq!(target, dst_dir.join("photo.png"));
    assert!(
        lumina_sidecar::sidecar_path_for(&target).is_file(),
        "the sidecar travels with the image"
    );
    assert_eq!(
        app.path,
        target.display().to_string(),
        "loaded path follows the move"
    );
    assert!(
        app.sidecar_revision.is_some(),
        "CAS anchor is the moved sidecar's revision"
    );
}

#[test]
fn g09_delete_empty_folder_refuses_non_empty() {
    let root = tempfile::tempdir().unwrap();
    let full = root.path().join("full");
    let empty = root.path().join("empty");
    std::fs::create_dir(&full).unwrap();
    std::fs::create_dir(&empty).unwrap();
    save_png(&full.join("photo.png"));
    let mut app = new_app();
    assert!(app.delete_empty_folder(&full).is_err());
    assert!(full.is_dir(), "non-empty folders are never deleted");
    app.delete_empty_folder(&empty).unwrap();
    assert!(!empty.exists());
    assert!(app.delete_empty_folder(&empty).is_err());
}
