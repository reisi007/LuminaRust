use super::*;

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness.run();
    harness.snapshot("library_empty");
}

#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_with_image() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.run();
    harness.snapshot("library_with_image");
}

/// LRPAR-G12-FACE-20 (S5): the Library People view (no analysis state) plus
/// the new Library view selector. Golden; `#[ignore]`d like every GPU snapshot.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_people_empty() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    load_sample(&mut harness);
    harness.state_mut().set_library_view(LibraryView::People);
    harness.run();
    assert_label_on_screen(&mut harness, "People");
    harness.snapshot("library_people_empty");
}

// ---------------------------------------------------------------------------
// F-103-N9 interaction tests (deterministic state assertions, no snapshots).
// Like the snapshots, these need a headless wgpu harness, so they are
// `#[ignore]`d and run with the `-- --ignored` flag on a GPU machine.
// ---------------------------------------------------------------------------

/// Create a temporary folder with `count` RAW **layout** sentinels.
///
/// GOLDEN-FIXT-31 (contract class S1): these files are not image fixtures and
/// are never committed. They exist only where the assertion is a *geometry* or
/// *count* that must hold independently of decode success (the filmstrip's
/// single-row layout with 20 cells), and the caller keeps a real loaded source
/// so the F-100 auto-load never decodes a sentinel. Any golden that asserts
/// pixels must use a staged real CR3 instead — see `kittest_fixtures_support`.
fn temp_raw_dir(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("temp dir");
    for i in 0..count {
        std::fs::write(
            dir.path().join(format!("IMG_{:04}.ARW", i)),
            b"lumina-raw-layout-sentinel",
        )
        .expect("write dummy raw");
    }
    dir
}

/// Collect the visible filmstrip cells and assert that they form exactly one
/// horizontal row.
///
/// Each chip is an egui clickable area that surfaces in the accesskit tree as
/// `Role::Unknown` with the full cell rect (`CELL_W x CELL_H` = 140x110) in the
/// bottom band (`y > 500` of the 720-high window). GUI-VIEW-2: the navigator
/// rail is open by default and shows its own 120x90 thumbnail column on the
/// left — those nodes match the band filter too, so require the 140-wide
/// filmstrip cell geometry to keep this a filmstrip-only assertion. The row
/// check verifies all laid-out cells share (nearly) one y and advance strictly
/// to the right — no wrapping/stacking. Returns the sorted chips so callers can
/// add their own count/selection guards.
fn assert_filmstrip_single_row(harness: &mut Harness<'_, LuminaApp>) -> Vec<eframe::egui::Rect> {
    let mut chips: Vec<eframe::egui::Rect> = harness
        .query_all_by(|n| n.role() == eframe::egui::accesskit::Role::Unknown)
        .filter(|n| n.accesskit_node().bounding_box().is_some())
        .map(|n| n.rect())
        .filter(|r| r.min.y > 500.0 && r.height() > 60.0 && r.width() > 130.0)
        .collect();
    assert!(
        chips.len() >= 2,
        "expected at least 2 visible filmstrip cells, got {chips:?}"
    );
    chips.sort_by(|a, b| a.min.x.partial_cmp(&b.min.x).unwrap());
    let row_y = chips[0].center().y;
    for (i, cell) in chips.iter().enumerate() {
        assert!(
            (cell.center().y - row_y).abs() < 12.0,
            "cell {i} is vertically offset: {:?} (row y = {row_y})",
            cell.center()
        );
        if i > 0 {
            assert!(
                cell.min.x > chips[i - 1].min.x,
                "cell {i} does not advance x: {:?} then {:?}",
                chips[i - 1].min,
                cell.min
            );
        }
    }
    chips
}

/// The filmstrip is the bottom `Panel::bottom`; this pins the single-row
/// geometry for a 20-dummy strip (KITTEST-COVERAGE-STATES-1).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn filmstrip_is_single_row_horizontal() {
    let dir = temp_raw_dir(20);
    let mut harness = build_harness();
    set_directory_and_settle(&mut harness, dir.path().display().to_string());
    harness.state_mut().set_module(Module::Develop);
    // The app keeps requesting repaints while thumbnail jobs are scheduled, so
    // `run()` would exceed max_steps; run a fixed number of frames instead.
    harness.run_steps(3);
    assert_filmstrip_single_row(&mut harness);
}

/// UX-SLICE-1 (UXG-09): the shared filmstrip header shows an "n of N" counter
/// driven by the strip selection. Headless state assertion (no golden): the
/// counter text is a real accesskit label.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn filmstrip_counter_reflects_selection() {
    let mut harness = build_harness();
    let (paths, _) = setup_library_views(&mut harness);
    // Exactly the first entry selected: 1 of 3.
    harness
        .state_mut()
        .select_filmstrip_path(paths[0].clone(), false, false);
    harness.run_steps(3);
    assert!(
        harness.query_all_by_label("1 of 3").next().is_some(),
        "one selected strip entry must render the 1-of-N counter"
    );
    // Toggle-add the second entry: 2 of 3.
    harness
        .state_mut()
        .select_filmstrip_path(paths[1].clone(), true, false);
    harness.run_steps(3);
    assert!(
        harness.query_all_by_label("2 of 3").next().is_some(),
        "two selected strip entries must render the 2-of-3 counter"
    );
}

/// UX-SLICE-2 (F2/F5): the empty-state CTA is genuinely wired to the native
/// folder picker (injected headless — no display server), not the old no-op
/// re-list of the current directory. A picked folder with a RAW entry proves
/// the CTA leaves the empty state and adopts the picked path.
///
/// GOLDEN-FIXT-31: the picked folder gets a **real** staged CR3. A RAW
/// sentinel would work for the listing assert, but it would raise the loud
/// LibRaw failure banner mid-test, so the frame budget below would race a
/// decode error that has nothing to do with the wiring under test.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_empty_state_cta_picks_folder() {
    let picked = tempfile::tempdir().expect("picked temp dir");
    let staged = stage_raw(picked.path(), "picked.cr3", "aircraft-landscape.cr3");
    std::fs::create_dir_all(picked.path().join("sub")).expect("create picked subdir");
    let picked_path = picked.path().display().to_string();

    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    let injected = picked_path.clone();
    harness
        .state_mut()
        .set_folder_picker(move || Some(PathBuf::from(injected.clone())));
    harness.run();
    assert!(
        harness.query_all_by_label("No images").next().is_some(),
        "empty state must show its title before the CTA is used"
    );
    let clicked = harness
        .query_all_by_label("Open Folder")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "empty-state CTA must be present and clickable");
    // Settle the picked folder's listing + auto-load decode before asserting
    // (GOLDEN-FIXT-31): a fixed frame budget cannot cover a real 24-megapixel
    // RAW decode, and the assertion below is about the settled listing.
    settle_scan(&mut harness);
    assert_eq!(
        harness.state_mut().directory(),
        picked_path,
        "CTA must adopt the folder returned by the picker (no-op re-list is not wiring)"
    );
    assert!(
        !harness.state_mut().entries().is_empty(),
        "the picked folder's RAW entry must be listed"
    );
    assert!(
        harness.query_all_by_label("No images").next().is_none(),
        "a non-empty picked folder must leave the empty state"
    );
    // The staged CR3 really is on disk where the listing found it (a listing
    // assert over a phantom file would be vacuous), and it really decoded.
    assert!(
        staged.is_file() && staged.extension().is_some_and(|ext| ext == "cr3"),
        "the picked folder must contain the staged RAW fixture"
    );
    assert_no_raw_decode_failure(&mut harness);
}

/// UX-SLICE-2 (F2): a cancelled folder dialog is a deliberate no-op — the
/// directory and the empty state stay exactly as they were.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_empty_state_cta_cancel_is_noop() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness.state_mut().set_folder_picker(|| None);
    harness.run();
    let before = harness.state_mut().directory().to_owned();
    let clicked = harness
        .query_all_by_label("Open Folder")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "empty-state CTA must be present and clickable");
    harness.run();
    assert_eq!(
        harness.state_mut().directory(),
        before,
        "a cancelled dialog must not change the directory"
    );
    assert!(
        harness.query_all_by_label("No images").next().is_some(),
        "a cancelled dialog must keep the empty state"
    );
}

/// UX-SLICE-2 (F3): Loupe/Compare/Survey use the same shared empty state as
/// the Grid (title + "Open Folder" CTA) instead of their former heading-only
/// text, so no view can drift into a divergent empty presentation.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn non_grid_library_views_share_the_empty_state() {
    for view in [
        LibraryView::Loupe,
        LibraryView::Compare,
        LibraryView::Survey,
    ] {
        let mut harness = build_harness();
        harness.state_mut().set_module(Module::Library);
        use_library_fixture(&mut harness);
        harness.state_mut().set_library_view(view);
        harness.run_steps(2);
        assert!(
            harness.query_all_by_label("No images").next().is_some(),
            "{view:?} must show the shared empty-state title"
        );
        assert!(
            harness.query_all_by_label("Open Folder").next().is_some(),
            "{view:?} must show the shared empty-state CTA"
        );
    }
}

/// GUI-DEBUG-SWEEP-1: the internal render hash is **no longer painted** as
/// header text (it was the last debug value in the header). It stays available
/// as a tooltip on the status line via `render_hash_tooltip`, which keeps the
/// exact gate `render_hash_visible` used. No heading/tooltip means the internal
/// key can never be part of the end-user header again.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn render_hash_is_tooltip_only() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Develop);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    assert!(
        harness.state_mut().render_hash_visible(),
        "a loaded Develop image must have a meaningful hash"
    );
    assert!(
        harness.state_mut().render_hash_tooltip().is_some(),
        "the hash must remain reachable as a status-line tooltip"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "the render hash must never be painted as visible text"
    );
}

/// UX-SLICE-2 (F1) / GUI-DEBUG-SWEEP-1: the render-hash tooltip is suppressed
/// in the Library module while the grid empty state is shown, so it can never
/// contradict "No images". UX-SLICE-3 (F1 follow-up): the gate tracks the
/// *filtered* raster the empty state itself uses — a `\` query with zero
/// matches suppresses the tooltip even though RAW entries are listed, and
/// clearing it restores both. The underlying `render_key` stays valid; the
/// tooltip returns with Develop.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_empty_suppresses_render_hash() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    assert!(
        harness.state_mut().render_key().is_some(),
        "the sample must have a current render"
    );
    // Unfiltered empty: the fixture directory has no RAW entries.
    assert!(
        !harness.state_mut().render_hash_visible(),
        "F1 gate must hide the hash while the RAW grid is empty"
    );
    assert!(
        harness.state_mut().render_hash_tooltip().is_none(),
        "no render hash tooltip may be offered above the Library empty state"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "no render hash may be painted above the Library empty state"
    );
    // Filtered empty (UX-SLICE-3): a RAW grid exists, but the `\` query
    // matches none, so the same shared empty state is shown and the gate must
    // hide the hash just like the unfiltered case.
    ensure_library_views_fixture();
    harness
        .state_mut()
        .set_directory(LIBRARY_VIEWS_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; mirror the views tests' recursive listing.
    list_directory_and_settle(&mut harness);
    // The staged CR3s must decode here too: the F-100 auto-load of the first
    // entry runs a full LibRaw decode, and a decode failure would raise the
    // loud error whose suppression this test is about.
    assert_no_raw_decode_failure(&mut harness);
    harness.state_mut().set_library_filter("no-such-entry");
    harness.run();
    assert!(
        harness.state_mut().render_key().is_some(),
        "the gate must be judged against a current render, not an absent one"
    );
    assert!(
        harness.state_mut().filtered_library_order().is_empty(),
        "the `\\` filter must yield a zero-match raster for this guard"
    );
    assert!(
        !harness.state_mut().render_hash_visible(),
        "a zero-match filter must hide the hash despite listed RAW entries"
    );
    assert!(
        harness.state_mut().render_hash_tooltip().is_none(),
        "a zero-match filter must suppress the render hash tooltip"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "no render hash may be painted above the filtered empty state"
    );
    // Clearing the filter restores the non-empty raster and the hash.
    harness.state_mut().set_library_filter("");
    harness.run();
    assert!(
        !harness.state_mut().filtered_library_order().is_empty(),
        "clearing the filter must restore the listed raster"
    );
    assert!(
        harness.state_mut().render_hash_visible(),
        "a non-empty filtered raster must keep the render hash"
    );
    assert!(
        harness.state_mut().render_hash_tooltip().is_some(),
        "the render hash tooltip must return on the non-empty raster"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "the hash must still never be painted as visible text"
    );
    // Same render, other module: the hash is meaningful again.
    harness.state_mut().set_module(Module::Develop);
    harness.run();
    assert!(
        harness.state_mut().render_hash_visible(),
        "Develop must keep the render hash for the loaded image"
    );
    assert!(
        harness.state_mut().render_hash_tooltip().is_some(),
        "the render hash tooltip must return in Develop"
    );
    assert!(
        harness
            .query_all_by_label_contains("Render state current:")
            .next()
            .is_none(),
        "the render hash must never be painted as visible text in Develop"
    );
}

/// The Library Folders tree must be rooted at the current workdir (the
/// `directory` field), not at `$HOME` — the tree root label is the workdir's
/// basename.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_folders_root_is_workdir() {
    let dir = tempfile::tempdir().expect("temp dir");
    // A subfolder ensures the tree has at least one child node so the root is
    // clearly distinguishable.
    std::fs::create_dir(dir.path().join("sub")).unwrap();
    let base = dir
        .path()
        .file_name()
        .and_then(|n| n.to_str())
        .expect("basename")
        .to_owned();

    let mut harness = build_harness();
    set_directory_and_settle(&mut harness, dir.path().display().to_string());
    harness.state_mut().set_module(Module::Library);
    harness.run_steps(3);

    // The workdir's basename must appear as a folder node label in the tree.
    let found = harness.query_all_by_label_contains(&base).next().is_some();
    assert!(
        found,
        "Folder tree must show the workdir `{base}` as its root (not $HOME)"
    );
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-LIBRARY-1: Library Loupe / Compare / Survey goldens.
//
// The default Library grid already has goldens (`library_empty`,
// `library_with_image`, `library_subfolder_badges`); the three remaining
// G-09 views get one golden each here, following the 68215fd/7fd528b/dd73806
// pattern: deterministic seeds, non-vacuous guard + on-screen assert, then
// `snapshot`. Two minimal production layout fixes were required along the
// way (B1: the folder-tree path row claimed the whole panel height via a
// direction-changing `with_layout`, hiding the tree; B2: the fixed Loupe
// height buried the rating line under the filmstrip) — the 9 pre-existing
// Library goldens pinning that broken layout were rebaselined with them
// (diffs limited to the fixed regions, verified per golden).
//
// Vision baselines covered:
// * `library_with_image` shows no thumbnail (grid center = empty state):
//   `library_loupe` proves real image content — the cells are painted by the
//   production thumbnail worker from a **real** CR3 decode (GOLDEN-FIXT-31), and
//   `settle_thumbnails` decodes the produced preview back to pin the real-pixel
//   path, so Loupe never paints the empty/placeholder text.
// * `subfolder_badges` cell texts squeezed/overlapping (minor): the views
//   fixture stays flat with short names (`a01.cr3` …), so no badge row can
//   overlap; the badges themselves stay pinned by `library_subfolder_badges`.
// ---------------------------------------------------------------------------

/// Committed fixture directory for the Loupe/Compare/Survey snapshots below
/// (same rationale as `LIBRARY_BADGES_FIXTURE_DIR`: a relative path keeps
/// every rendered string fixed; a `tempfile::tempdir` would leak its random
/// prefix into the folder-tree + path-field pixels). Nothing binary is
/// committed here: the three CR3s are staged from `sample-data/raw/` per run.
const LIBRARY_VIEWS_FIXTURE_DIR: &str = "tests/fixtures/library_views";

/// Staged RAW files (flat, short names — see the badge note above) and the
/// committed fixture each is staged from. GOLDEN-FIXT-31 replaced 18-byte
/// sentinels plus seeded synthetic gradients with these real CR3s, so the
/// Survey view shows three genuinely decoded thumbnails.
const LIBRARY_VIEWS_FILES: &[(&str, &str)] = &[
    ("a01.cr3", "aircraft-portrait.cr3"),
    ("a02.cr3", "aircraft-landscape.cr3"),
    ("b01.cr3", "aircraft-portrait.cr3"),
];

/// Serializes the (re-)staging below: the views tests run in one process on
/// threads and share the same fixture files, and the thumbnail worker itself
/// stages cache records through a pid-named temp file — concurrent staging of
/// the same entry would race on that temp path.
static LIBRARY_VIEWS_FIXTURE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// (Re-)stage the real RAW files and pre-create the folder cache. Idempotent:
/// called on every run. Deliberately stores **no** preview: the cells must be
/// painted by the production thumbnail worker from a real decode, otherwise a
/// stale cache hit could keep a broken decode green (GOLDEN-FIXT-31).
fn ensure_library_views_fixture() -> Vec<StagedEntry> {
    let _guard = LIBRARY_VIEWS_FIXTURE_LOCK
        .lock()
        .expect("views fixture lock");
    let root = Path::new(LIBRARY_VIEWS_FIXTURE_DIR);
    let _ = std::fs::remove_dir_all(root.join(".lumina"));
    let entries: Vec<StagedEntry> = LIBRARY_VIEWS_FILES
        .iter()
        .map(|&(name, source)| (stage_raw(root, name, source), source))
        .collect();
    prepare_folder_cache(root);
    entries
}

/// Assert that a label *containing* `needle` is laid out inside the
/// 1024x720 window (same rationale as `assert_label_on_screen`, but for
/// composite labels like the Loupe `name [status]` line whose status suffix
/// is not worth hardcoding).
///
/// NOTE (B1 lesson): a contains-query can match a *different* widget than
/// the intended one (the path text field also contains the fixture dir, a
/// covered line is still "on-screen" for accesskit). Guards for tree nodes
/// and the rating line therefore use exact labels (below) plus a coverage
/// assert — never a bare contains.
fn assert_contains_on_screen(harness: &mut Harness<'_, LuminaApp>, needle: &str) {
    let rect = harness
        .query_all_by_label_contains(needle)
        .next()
        .unwrap_or_else(|| panic!("label containing {needle:?} not found in headed harness"))
        .rect();
    assert!(
        rect.min.y >= 0.0 && rect.max.y <= 720.0 && rect.max.x <= 1024.0,
        "label containing {needle:?} must be pixel-visible in the 1024x720 viewport, got {rect:?}"
    );
}

/// B1 guard: the folder-tree root node, matched EXACTLY (the path text
/// field carries the same directory string, so a contains-query passes
/// vacuously on the field while the tree stays invisible). The root is
/// expanded by production code every frame (`open_folders.insert`), so an
/// exact on-screen match proves a pixel-visible tree node — no test scroll
/// involved.
fn assert_tree_root_on_screen(harness: &mut Harness<'_, LuminaApp>) {
    assert_label_on_screen(harness, "library_views (3)");
}

/// B2 guard: the Loupe rating line must paint strictly above the bottom
/// filmstrip panel. An accesskit on-screen rect alone does not prove it —
/// the line used to sit underneath the filmstrip while still reporting an
/// in-viewport rect (vacuous pass, invisible pixels).
fn assert_rating_above_filmstrip(harness: &mut Harness<'_, LuminaApp>) {
    let rating = harness
        .query_all_by_label_contains("Rating:")
        .next()
        .unwrap_or_else(|| panic!("rating line not found in headed harness"))
        .rect();
    let filmstrip = harness
        .query_all_by_label("Filmstrip")
        .next()
        .unwrap_or_else(|| panic!("filmstrip heading not found in headed harness"))
        .rect();
    assert!(
        rating.max.y <= filmstrip.min.y,
        "rating line {rating:?} must sit above the filmstrip (top {})",
        filmstrip.min.y
    );
}

/// Point a Library harness at the views fixture with a recursive listing.
///
/// Returns the three RAW display-string paths in raster order. Asserts the
/// relative directory (no tempdir-prefix leakage into folder-tree /
/// path-field pixels) plus the 3-file RAW order the views below share.
fn setup_library_views(harness: &mut Harness<'_, LuminaApp>) -> (Vec<String>, Vec<StagedEntry>) {
    let staged = ensure_library_views_fixture();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_VIEWS_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the recursive aggregation is the views'
    // shared order (identical here — the fixture is flat).
    list_directory_and_settle(harness);
    assert_eq!(
        harness.state_mut().directory(),
        LIBRARY_VIEWS_FIXTURE_DIR,
        "views must render the relative fixture dir (no tmp-prefix pixels)"
    );
    assert_eq!(
        harness.state_mut().entries().len(),
        3,
        "views fixture must list all files"
    );
    let order = harness.state_mut().filtered_library_order();
    assert_eq!(order.len(), 3, "RAW-only order must see all files");
    let mut keys: Vec<String> = harness
        .state_mut()
        .entries()
        .iter()
        .map(|entry| entry.thumb_key().to_owned())
        .collect();
    keys.sort();
    assert!(keys[0].ends_with("a01.cr3"), "unexpected key {}", keys[0]);
    assert!(keys[1].ends_with("a02.cr3"), "unexpected key {}", keys[1]);
    assert!(keys[2].ends_with("b01.cr3"), "unexpected key {}", keys[2]);
    // Display-string paths (`dir/name`, the unit `select_filmstrip_path`
    // compares against): the flat fixture joins deterministically. A wrong
    // assumption fails loudly at the selection asserts below, never silent.
    let mut paths: Vec<String> = LIBRARY_VIEWS_FILES
        .iter()
        .map(|(name, _)| format!("{LIBRARY_VIEWS_FIXTURE_DIR}/{name}"))
        .collect();
    paths.sort();
    (paths, staged)
}

/// Loupe (`E`): the active selection shown large with real thumbnail pixels
/// (real CR3 decode, see above) plus the rating line. The rating line doubles
/// as the "Rating-Sektion im Grid-Kontext"/w visible rating UI of the
/// Library module (the Develop rating section has its own golden).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_loupe() {
    let mut harness = build_harness();
    let (_, staged) = setup_library_views(&mut harness);
    // A real in-memory decode alongside (Preview-Generations-Assert): proves
    // genuine image content flows while the Loupe view paints thumbnails.
    load_sample(&mut harness);
    harness.run();
    assert!(
        harness.state_mut().preview_generation() >= 1,
        "loaded sample must render at least once"
    );
    harness.state_mut().set_library_view(LibraryView::Loupe);
    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &staged);
    assert_no_raw_decode_failure(&mut harness);
    // Non-vacuous guards: the Loupe heading, the active file line, the
    // folder-tree root node (B1: exact match — a contains-query passes
    // vacuously on the path text field) and the rating line above the
    // filmstrip (B2) must actually be pixel-visible — otherwise the golden
    // below could pass on the empty-state text the Vision baseline flagged.
    assert_label_on_screen(&mut harness, "Loupe (E): single image");
    assert_contains_on_screen(&mut harness, "a01.cr3");
    assert_tree_root_on_screen(&mut harness);
    assert_contains_on_screen(&mut harness, "Rating:");
    assert_rating_above_filmstrip(&mut harness);
    harness.snapshot("library_loupe");
}

/// Compare (`C`): Before/After of the active image side by side (the same real
/// CR3 thumbnail texture twice, `before_after` held by `set_library_view`).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_compare() {
    let mut harness = build_harness();
    let (_, staged) = setup_library_views(&mut harness);
    harness.state_mut().set_library_view(LibraryView::Compare);
    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &staged);
    assert_no_raw_decode_failure(&mut harness);
    // Non-vacuous guards: the Compare heading, both pane labels, the
    // live status line (proves the `before_after` Compare branch, not the
    // empty state) and the folder-tree root node (B1: exact match) must
    // actually be on-screen.
    assert_label_on_screen(&mut harness, "Compare");
    assert_label_on_screen(&mut harness, "Before");
    assert_label_on_screen(&mut harness, "After");
    assert_contains_on_screen(&mut harness, "Compare view on (Compare)");
    assert_tree_root_on_screen(&mut harness);
    harness.snapshot("library_compare");
}

/// Survey (`N`): the multi-selection side by side (all three staged files
/// selected, so the real multi-selection branch renders — not the
/// below-two fallback raster). Folder tree (left) and the expanded keyword
/// chips (right Metadata panel) are part of this golden: the grid-context
/// companions the task requires.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn library_survey() {
    let mut harness = build_harness();
    let (paths, staged) = setup_library_views(&mut harness);
    // Select all three (first plain, rest toggle-add): proves the genuine
    // multi-selection branch. A wrong path is a loud no-op in
    // `apply_filmstrip_click`, so the length assert below guards the join
    // assumption in `setup_library_views` too.
    harness
        .state_mut()
        .select_filmstrip_path(paths[0].clone(), false, false);
    harness
        .state_mut()
        .select_filmstrip_path(paths[1].clone(), true, false);
    harness
        .state_mut()
        .select_filmstrip_path(paths[2].clone(), true, false);
    let selected = harness.state_mut().filmstrip_selection();
    assert_eq!(selected.len(), 3, "survey needs all three files selected");
    for path in &paths {
        assert!(
            selected.contains(path),
            "selection must contain {path} (got {selected:?})"
        );
    }
    harness.state_mut().set_library_view(LibraryView::Survey);
    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &staged);
    assert_no_raw_decode_failure(&mut harness);
    // Non-vacuous guards: heading + live count (proves the multi-selection
    // branch, not the fallback) and the folder-tree root node (B1: exact
    // match, not the path-field contains) as grid context.
    assert_label_on_screen(&mut harness, "Survey");
    assert_label_on_screen(&mut harness, "3 selected");
    assert_tree_root_on_screen(&mut harness);
    // Keyword chips (right Metadata panel): expand the section so the golden
    // pins the input row, not just the collapsed header. Same scroll-first
    // pattern as the metadata subpanels (a below-fold click is discarded).
    harness.run();
    let header_visible = harness
        .query_all_by_label("Keywords")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        header_visible,
        "Keywords header not found in headed harness"
    );
    harness.run();
    harness.run();
    let clicked = harness
        .query_all_by_label("Keywords")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(clicked, "Keywords header lost after scrolling");
    harness.run();
    let found = harness
        .query_all_by_label("Add keyword")
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(found, "keyword input row missing after expanding");
    harness.run();
    harness.run();
    assert_label_on_screen(&mut harness, "Add keyword");
    // Kosmetik (c/K1): park the pointer fully outside the frame so the
    // rendered cursor cannot leave a tip on the golden (an in-frame park
    // position still showed a 1–2px arrow tip). Out-of-viewport positions
    // are clipped by the renderer and stay invisible.
    harness.hover_at(eframe::egui::Pos2::new(2000.0, 2000.0));
    harness.run_steps(2);
    harness.snapshot("library_survey");
}

/// Filmstrip with 20 layout sentinels: the single-row geometry (asserted, see
/// `assert_filmstrip_single_row`) plus a pixel golden. The bundled sample is
/// loaded first so the F-100 auto-load never decodes an invalid sentinel (which
/// would raise the error dialog); the strip still lists all 20 RAW cells.
///
/// GOLDEN-FIXT-31: the committed golden pins the **per-cell failure text** of
/// the S1 layout sentinels. That text only appears once the thumbnail worker has
/// reported the failed decode, so a fixed frame budget is a race — under load
/// (the R1 fixtures make the whole binary slower) the cells were still blank at
/// snapshot time. The wait below makes the state deterministic instead of
/// load-dependent; the rendered image is unchanged.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored --test-threads=1"]
fn filmstrip_twenty_dummies() {
    let dir = temp_raw_dir(20);
    let mut harness = build_harness();
    // Load a valid source first: suppresses the RAW auto-load (which would
    // fail loudly on the sentinel bytes) and gives the preview real pixels.
    load_sample(&mut harness);
    set_directory_and_settle(&mut harness, dir.path().display().to_string());
    harness.state_mut().set_module(Module::Develop);
    // Wait for the sentinel cells' failure state (fixed frames, not `run()`:
    // thumbnail jobs keep requesting repaints).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while harness
        .query_all_by_label_contains("LibRaw opening input failed")
        .next()
        .is_none()
    {
        assert!(
            std::time::Instant::now() < deadline,
            "the sentinel cells never reported their decode failure"
        );
        harness.run_steps(2);
    }
    // Single-row geometry for all visible cells.
    let chips = assert_filmstrip_single_row(&mut harness);
    assert!(
        chips.len() >= 2,
        "20-dummy strip must lay out several cells, got {chips:?}"
    );
    // Non-vacuous guard: the strip knows all 20 entries (selection resolved to
    // the first entry, so the header shows "1 of 20").
    assert_contains_on_screen(&mut harness, "of 20");
    harness.snapshot("filmstrip_twenty_dummies");
}
