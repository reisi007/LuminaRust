use super::*;

/// LRPAR-G15-IPTC-S8: the Library right-column Metadata panel (draft
/// editor expanded). The presets directory is overridden with an empty
/// tempdir so no machine-global preset names leak into the golden (the path
/// itself is never rendered, only preset names — determinism holds).
/// `load_sample` is path-less, so embedded reads stay on the deterministic
/// "unavailable" note.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_metadata() {
    let presets = tempfile::tempdir().expect("temp dir");
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    load_sample(&mut harness);
    harness.run();
    // Second layout frame before querying: the first frame after
    // `load_bytes` settles the image load, only the second lays out the
    // right-column panel headers, so the `collapsing` toggle exists when
    // clicked (same reason `collapse_except` runs before its first query).
    harness.run();
    // Expand the draft editor so the golden pins the field rows, not just
    // the collapsed headers. The label is unique to this section (the
    // preview "Draft" badge uses a different string).
    let clicked = harness
        .query_all_by_label("Metadata draft")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Metadata draft section not found in headed harness"
    );
    harness.run();
    // Non-vacuous guard: the expanded editor must expose its field rows
    // ("Date created" is unique to the draft editor); otherwise the golden
    // below could pass on a collapsed panel without any editor pixels.
    assert!(
        harness.query_all_by_label("Date created").next().is_some(),
        "draft editor did not expand: field row 'Date created' missing"
    );
    harness.snapshot("library_metadata");
}
///
/// Committed layout (`top.cr3` + `sub/mid.cr3` + `sub/nested/deep.cr3`), with
/// the three RAW files staged at run time from `sample-data/raw/` (see
/// `kittest_fixtures_support`); nothing binary is committed here. The relative
/// path keeps every rendered string fixed, and the relative badges (`""`,
/// `"sub"`, `"sub/nested"`) are machine-independent — unlike a
/// `tempfile::tempdir` path, which would leak nondeterministic pixels into the
/// golden via the path field and tree root label.
const LIBRARY_BADGES_FIXTURE_DIR: &str = "tests/fixtures/library_badges";

/// (Re-)stage the deterministic badge fixture (idempotent).
///
/// GOLDEN-FIXT-31: the three staged files are **real** CR3 fixtures. They used
/// to be 18-byte RAW sentinels, so the grid painted the deterministic LibRaw
/// "opening input failed" placeholder and this golden could not see a
/// single pixel of image-pipeline output. The relative-path badges
/// (`""` / `sub` / `sub/nested`) remain the regression signal under test, and
/// they are now painted over real decoded thumbnails.
fn ensure_library_badges_fixture() -> Vec<StagedEntry> {
    let root = std::path::Path::new(LIBRARY_BADGES_FIXTURE_DIR);
    std::fs::create_dir_all(root.join("sub/nested")).expect("create badge fixture dirs");
    // Remove stale pre-RAW fixtures (H1 first attempt used `.png`).
    for stale in [
        "tests/fixtures/library_badges/top.png",
        "tests/fixtures/library_badges/sub/mid.png",
        "tests/fixtures/library_badges/sub/nested/deep.png",
    ] {
        let _ = std::fs::remove_file(stale);
    }
    // The grid's thumbnail probe creates a gitignored `.lumina/` preview cache
    // inside the fixture tree; left behind it would appear as an extra folder
    // row on the *next* run and make this golden order-dependent. Removing it
    // makes every run start from the same file set (fresh checkout and re-run
    // render identically).
    for cache in [
        "tests/fixtures/library_badges/.lumina",
        "tests/fixtures/library_badges/sub/.lumina",
        "tests/fixtures/library_badges/sub/nested/.lumina",
    ] {
        let _ = std::fs::remove_dir_all(cache);
    }
    let entries: Vec<StagedEntry> = [
        ("top.cr3", "aircraft-landscape.cr3"),
        ("sub/mid.cr3", "aircraft-portrait.cr3"),
        ("sub/nested/deep.cr3", "aircraft-landscape.cr3"),
    ]
    .into_iter()
    .map(|(staged, source)| (stage_raw(root, staged, source), source))
    .collect();
    // Pre-create the per-folder caches so the folder tree is identical on a
    // cold and a warm run.
    for folder in [root, &root.join("sub"), &root.join("sub/nested")] {
        prepare_folder_cache(folder);
    }
    entries
}

/// GUI-LIBRARY-SUBFOLDERS-1: the Library grid aggregates subfolders with a
/// visible relative-path badge (`sub`, `sub/nested`; empty for top-level).
/// Golden regression for the badge state (H1).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_subfolder_badges() {
    let entries = ensure_library_badges_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_BADGES_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the recursive aggregation carries badges.
    list_directory_and_settle(&mut harness);
    // Non-vacuous guard: the recursive listing (not the flat one) must see
    // all three files, otherwise the golden below could pass on an empty
    // grid without any badge pixels. (`FileBrowserEntry` fields are
    // private; `thumb_key()` is the public per-entry path accessor.)
    let listed = harness.state_mut().entries();
    assert_eq!(listed.len(), 3, "recursive aggregation must see all files");
    let mut keys: Vec<String> = listed
        .iter()
        .map(|entry| entry.thumb_key().to_owned())
        .collect();
    keys.sort();
    assert!(keys[0].ends_with("mid.cr3"), "unexpected key {}", keys[0]);
    assert!(keys[1].ends_with("deep.cr3"), "unexpected key {}", keys[1]);
    assert!(keys[2].ends_with("top.cr3"), "unexpected key {}", keys[2]);
    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &entries);
    assert_no_raw_decode_failure(&mut harness);
    harness.snapshot("library_subfolder_badges");
}

// ---------------------------------------------------------------------------
// UX-SLICE-2 (F4): the LR-01 rating/flag/color-label badge is painted by the
// shared `paint_entry_badge` helper. The pre-existing "badge" fixtures are all
// unrated, so no badge was ever painted; this fixture carries exactly one
// rated/picked/red, one rejected/blue and one unrated/green entry (badge text
// asserted through the public `FileBrowserEntry::badge_text` accessor) and the
// golden plus an exact-`LIBRARY_BADGE_BG` pixel count prove the chip pixels.
// ---------------------------------------------------------------------------

const LIBRARY_RATED_FIXTURE_DIR: &str = "tests/fixtures/library_rated";

/// (Re-)stage a deterministic, conflict-free rated fixture: three **real** CR3
/// fixtures whose sidecars carry the default copy's rating/flag/label.
///
/// GOLDEN-FIXT-31: the three staged files used to be 18-byte RAW sentinels with
/// a *seeded* synthetic gradient in the preview cache, so the cells painted a
/// colour ramp and the F-100 auto-load of `labeled.arw` raised the deterministic
/// LibRaw "opening input failed" banner. Now the sidecars describe the real
/// staged bytes (content hash, LibRaw decoder identity, decoded geometry) and
/// the cells are painted by the production thumbnail worker from a real decode.
/// The `.lumina/` node is pre-created (same on-disk shape as before) so the
/// folder tree is stable, but **no preview is seeded** — a cache hit would hide
/// a broken decode. The sidecar content hash matches the staged bytes
/// (`source_status` => `Unchanged`), so the golden pins the badges, never a
/// conflict state. Idempotent.
fn ensure_library_rated_fixture() -> Vec<StagedEntry> {
    use lumina_sidecar::{Flag, SidecarDocument};
    let root = Path::new(LIBRARY_RATED_FIXTURE_DIR);
    std::fs::create_dir_all(root).expect("create rated fixture dir");
    // Fresh cache directory before re-staging, so every run starts from the
    // same file set (no stale preview entries).
    let _ = std::fs::remove_dir_all(root.join(".lumina"));
    let mut entries: Vec<StagedEntry> = Vec::new();
    for (name, source, rating, flag, label) in [
        ("rated.cr3", "aircraft-landscape.cr3", 5u8, Flag::Pick, 1u64),
        ("rejected.cr3", "aircraft-portrait.cr3", 2, Flag::Reject, 4),
        (
            "labeled.cr3",
            "aircraft-landscape.cr3",
            0,
            Flag::Unflagged,
            3,
        ),
    ] {
        entries.push((stage_raw(root, name, source), source));
        let identity = staged_source_identity(name, source);
        let mut document = SidecarDocument::new(identity, "raster-mvp-1");
        document.virtual_copies[0].rating = rating;
        document.virtual_copies[0].flag = flag;
        document.virtual_copies[0]
            .extras
            .insert("color_label".to_owned(), serde_json::Value::from(label));
        let sidecar = lumina_sidecar::sidecar_path_for(&root.join(name));
        lumina_sidecar::save_sidecar(&sidecar, &document).expect("seed rated sidecar");
    }
    prepare_folder_cache(root);
    entries
}

/// Non-vacuous pixel guard for the F4 badge golden: count framebuffer pixels
/// equal to the shared chip fill [`LIBRARY_BADGE_BG`]. The fill is not part of
/// the theme palette and this flat fixture has no folder badges, so the only
/// source is `paint_entry_badge`. One 118x16 chip is ~1.9k raw pixels (minus
/// the glyphs); Grid + filmstrip together paint six, so `>= 2_000` is a
/// conservative lower bound that a clean (unrated) fixture cannot reach.
fn assert_badge_chips_painted(harness: &mut Harness<'_, LuminaApp>) {
    let rendered = harness.render().expect("kittest renders the frame");
    let (r, g, b) = (
        LIBRARY_BADGE_BG.r(),
        LIBRARY_BADGE_BG.g(),
        LIBRARY_BADGE_BG.b(),
    );
    let count = rendered
        .pixels()
        .filter(|pixel| {
            let [pr, pg, pb, _a] = pixel.0;
            pr == r && pg == g && pb == b
        })
        .count();
    assert!(
        count >= 2_000,
        "rated cells must paint the shared badge chip fill; got {count} exact-`LIBRARY_BADGE_BG` pixels"
    );
}

/// UX-SLICE-2 (F4): rated/flagged/labeled Library fixture — Grid cells and
/// filmstrip cells both paint the LR-01 badge, proven by the golden plus the
/// exact-fill pixel count. Badge text is asserted through the public accessor.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_rated_badges() {
    let entries = ensure_library_rated_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    harness
        .state_mut()
        .set_directory(LIBRARY_RATED_FIXTURE_DIR.to_owned());
    // `set_directory` lists flat; the grid's shared order is RAW-only.
    list_directory_and_settle(&mut harness);
    let mut badges: Vec<String> = harness
        .state_mut()
        .entries()
        .iter()
        .filter_map(|entry| entry.badge_text())
        .collect();
    badges.sort();
    assert_eq!(
        badges,
        vec![
            "★★★★★ P ●Red".to_owned(),
            "★★☆☆☆ X ●Blue".to_owned(),
            "☆☆☆☆☆ ●Green".to_owned(),
        ],
        "rated fixture must carry the three distinct badges (rating/flag/label)"
    );
    // Wait for the real CR3 thumbnails instead of a fixed frame budget.
    settle_thumbnails(&mut harness, &entries);
    assert_no_raw_decode_failure(&mut harness);
    // Non-vacuous pixel assert on the frame that is snapshotted below: a
    // mis-seeded (clean) fixture paints no chip fill and cannot reach the
    // threshold.
    assert_badge_chips_painted(&mut harness);
    harness.snapshot("library_rated_badges");
}

// ---------------------------------------------------------------------------
// KITTEST-COVERAGE-META-1: Library Metadata subpanels + preset dialog.
//
// The draft editor already has a golden (`library_metadata` above); the four
// remaining subpanels (Embedded / History / Preset / Sync) get one expanded
// golden each (exactly one open, the rest collapsed), plus one golden with
// the dynamic-preset prompt dialog open. Pattern per test (68215fd/7fd528b):
// deterministic seeds, header click + scroll-into-view, non-vacuous
// accesskit guard + on-screen assert, then `snapshot`. No production code is
// touched; existing goldens are not rebaselined.
// ---------------------------------------------------------------------------

/// Write `photo.jpg` with embedded IPTC into a fresh tempdir on the fly
/// (no repo binary). Returns the dir (keep alive until the snapshot is
/// taken) and the image path. The file name is fixed so the `Loaded:
/// photo.jpg` status line stays deterministic; the random tempdir prefix
/// never reaches pixels (see `open_file_and_restore_fixture`).
fn embedded_jpeg_fixture(title: &str, keywords: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("temp dir");
    let meta = lumina_iptc::IptcMetadata {
        title: Some(title.to_owned()),
        keywords: keywords.iter().map(|name| name.to_string()).collect(),
        ..Default::default()
    };
    let embedded =
        lumina_iptc::embed_metadata(&test_jpeg_bytes(), &meta).expect("embed IPTC fixture");
    let path = dir.path().join("photo.jpg");
    std::fs::write(&path, embedded).expect("write jpeg fixture");
    (dir, path)
}

/// Rendered History line for a seeded entry (`MetadataHistoryEntryPattern`).
fn history_line(rev: u64, day: u32) -> String {
    format!("rev {rev} | 2026-01-{day:02}T12:00:00Z | gui | title")
}

/// Write a dynamic meta preset file (one placeholder) into `dir`.
fn write_dynamic_meta_preset(dir: &Path, file: &str, name: &str) {
    let preset = serde_json::json!({
        "format": "lumina-meta-preset",
        "version": 1,
        "name": name,
        "fields": { "title": "{event}" },
        "placeholders": [{ "name": "event", "description": "Event name" }],
    });
    std::fs::write(dir.join(file), serde_json::to_vec_pretty(&preset).unwrap())
        .expect("write preset");
}

/// Click one Library Metadata collapsing header (all default closed) and
/// scroll a content label into view. The header itself is scrolled into
/// view first — a below-fold click would be discarded (7fd528b optics2
/// lesson) — with the same 2-frame settle as `expand_and_scroll_to`.
fn open_meta_section_and_scroll_to(
    harness: &mut Harness<'_, LuminaApp>,
    header_label: &str,
    target_label: &str,
) {
    harness.run();
    let header_visible = harness
        .query_all_by_label(header_label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        header_visible,
        "Metadata header {header_label:?} not found in headed harness"
    );
    harness.run();
    harness.run();
    let clicked = harness
        .query_all_by_label(header_label)
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(
        clicked,
        "Metadata header {header_label:?} lost after scrolling"
    );
    harness.run();
    let found = harness
        .query_all_by_label(target_label)
        .next()
        .map(|node| {
            node.scroll_to_me();
            true
        })
        .unwrap_or(false);
    assert!(
        found,
        "scroll target {target_label:?} not found in headed harness (header {header_label})"
    );
    harness.run();
    harness.run();
}

/// Embedded subpanel expanded (read-only JPEG IPTC): the keywords line is
/// the section-unique content label. Draft stays collapsed, so its
/// per-field `Embedded: …` overlay labels are absent.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_embedded() {
    let (_tmp, photo) = embedded_jpeg_fixture("Eingebettet", &["k1", "k2"]);
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    open_file_and_restore_fixture(&mut harness, &photo, ready_rendered_with_embedded_iptc());
    // Non-vacuous in-memory guard: the file really carries IPTC.
    let meta = harness
        .state_mut()
        .embedded_metadata()
        .unwrap()
        .expect("JPEG carries IPTC");
    assert_eq!(meta.title.as_deref(), Some("Eingebettet"));
    assert_eq!(meta.keywords, vec!["k1".to_owned(), "k2".to_owned()]);
    open_meta_section_and_scroll_to(
        &mut harness,
        "Embedded (read-only)",
        "Embedded keywords: k1, k2",
    );
    // Non-vacuous guard: the keywords line must actually be on-screen,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Embedded keywords: k1, k2");
    harness.snapshot("library_meta_embedded");
}

/// History subpanel expanded with 10 fixed-timestamp entries (newest
/// first). The scroll target is the oldest row at the bottom.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_history() {
    let dir = tempfile::tempdir().expect("temp dir");
    let photo = seed_metadata_history_sidecar(dir.path(), |_| {});
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    open_file_and_restore_fixture(&mut harness, &photo, ready_history_of(10));
    // Non-vacuity guard (KITT-IDENTITY-49): the 10 entries exist only because
    // the seeded sidecar passed the production source-identity validation in
    // `finish_decode`. A refused sidecar leaves an error banner and no
    // document, so an error here means the fixture is stale — never a golden
    // that quietly shows an empty History panel.
    assert!(
        harness.state().error().is_none(),
        "the seeded sidecar must be adopted, not refused as stale: {:?}",
        harness.state().error()
    );
    // Non-vacuous in-memory guard: newest-first order with fixed labels.
    // (Row-label existence is asserted after expanding below: collapsed
    // section content has no accesskit nodes.)
    let history = harness.state_mut().metadata_history();
    assert_eq!(history.len(), 10);
    assert_eq!(history[0].rev, 10);
    assert_eq!(history[9].rev, 1);
    let oldest = history_line(1, 1);
    let newest = history_line(10, 10);
    open_meta_section_and_scroll_to(&mut harness, "History", &oldest);
    assert!(
        harness.query_all_by_label(newest.as_str()).next().is_some(),
        "newest history row missing"
    );
    // Non-vacuous guard: the oldest row must actually be on-screen.
    assert_label_on_screen(&mut harness, &oldest);
    harness.snapshot("library_meta_history");
}

/// Preset subpanel expanded with an empty override dir (same pattern as
/// `library_metadata`): no machine-global preset name leaks into the
/// golden — the path itself is never rendered, only preset names.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_preset() {
    let presets = tempfile::tempdir().expect("temp dir");
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // Non-vacuous in-memory guard: the override really yields no presets.
    assert!(
        harness.state_mut().meta_preset_names().is_empty(),
        "empty override dir must list no presets"
    );
    open_meta_section_and_scroll_to(&mut harness, "Meta preset", "Apply preset");
    // Non-vacuous guard: the action row must actually be on-screen,
    // otherwise the golden below could pass on a collapsed header.
    assert_label_on_screen(&mut harness, "Apply preset");
    harness.snapshot("library_meta_preset");
}

/// Sync subpanel expanded: all 12 field rows (11 draft fields + keywords)
/// render checked by default (`default_meta_sync_fields`; the checked
/// pixels are the golden's signal, the Vision check confirms them).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_meta_sync() {
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    load_sample(&mut harness);
    harness.run();
    harness.run();
    // At click time only the header carries this label (the action button
    // with the same text appears after expanding).
    open_meta_section_and_scroll_to(&mut harness, "Sync to selection", "Title");
    // Non-vacuous guard: every sync row must exist (draft stays collapsed,
    // so these labels are unique to the sync section).
    for label in [
        "Title",
        "Headline",
        "Description",
        "Copyright",
        "Creator",
        "Credit",
        "Source",
        "City",
        "State / Province",
        "Country",
        "Date created",
        "Keywords",
    ] {
        assert!(
            harness.query_all_by_label(label).next().is_some(),
            "sync row {label:?} missing"
        );
    }
    assert_label_on_screen(&mut harness, "Title");
    harness.snapshot("library_meta_sync");
}

/// Dynamic-preset prompt dialog open with its required field
/// (`draw_meta_preset_dialog` draws it as a floating window from `update`).
/// The preset is selected through the combo UI (no private state poking);
/// after the snapshot Cancel is clicked headless and the no-write path is
/// asserted on disk (dialog-cancel state semantics themselves are covered
/// by the lib `iptc_gui_dynamic_preset_requires_vars` test).
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn draw_meta_preset_dialog() {
    let dir = tempfile::tempdir().expect("temp dir");
    let png = dir.path().join("photo.png");
    std::fs::write(&png, LuminaApp::sample_image_png()).expect("write png");
    let presets = tempfile::tempdir().expect("temp dir");
    write_dynamic_meta_preset(
        presets.path(),
        "KittestDialog.lumina-meta-preset.json",
        "KittestDialog",
    );
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    use_library_fixture(&mut harness);
    harness
        .state_mut()
        .set_meta_presets_dir(Some(presets.path().to_path_buf()));
    open_file_and_restore_fixture(&mut harness, &png, ready_rendered());
    // Non-vacuous in-memory guard: the dynamic preset is listed. The
    // panel auto-refresh runs inside the (closed) section closure, so an
    // explicit refresh is needed before the section is opened below.
    harness.state_mut().refresh_meta_presets();
    assert_eq!(
        harness.state_mut().meta_preset_names(),
        vec!["KittestDialog".to_owned()]
    );
    open_meta_section_and_scroll_to(&mut harness, "Meta preset", "Apply preset");
    // Select the preset through the combo UI: egui exposes the ComboBox as
    // a `ComboBox`-role node carrying the selected text as its *value*
    // (not its label), so it is queried by value. Before the selection the
    // value is still the hint, keeping the popup item unambiguous.
    let combo_open = harness
        .query_all_by_value("Choose a preset…")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(combo_open, "preset combo not found in headed harness");
    harness.run();
    harness.run();
    let item_picked = harness
        .query_all_by_label("KittestDialog")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(item_picked, "preset popup item not found in headed harness");
    harness.run();
    harness.run();
    let apply_clicked = harness
        .query_all_by_label("Apply preset")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(apply_clicked, "Apply preset button lost after selection");
    harness.run();
    harness.run();
    // Non-vacuous guard: the dialog window with its required field must
    // actually be on-screen.
    assert!(
        harness
            .query_all_by_label("Preset variables (all required)")
            .next()
            .is_some(),
        "preset dialog did not open"
    );
    assert_label_on_screen(&mut harness, "Preset variables (all required)");
    assert_label_on_screen(&mut harness, "Event name");
    harness.snapshot("draw_meta_preset_dialog");
    // Cancel path headless: the dialog closes and no sidecar is written.
    let cancel_clicked = harness
        .query_all_by_label("Cancel")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(cancel_clicked, "dialog Cancel button not found");
    harness.run();
    assert!(
        harness
            .query_all_by_label("Preset variables (all required)")
            .next()
            .is_none(),
        "dialog must close on Cancel"
    );
    assert!(
        !lumina_sidecar::sidecar_path_for(&png).is_file(),
        "cancelled dialog must not write a sidecar"
    );
}

/// Library metadata panel's own copy/paste system: a draft with entered values,
/// copied through the panel's Copy button (status "Metadata copied (2 field(s))")
/// and the Paste affordance pixel-visible. Uses a real path so the commit lands
/// in a tempdir only; `open_file_and_restore_fixture` keeps tmp prefixes out of
/// the golden.
#[test]
#[ignore = "headless GPU required; run: cargo test -p lumina-gui --test kittest_snapshots -- --ignored"]
fn library_metadata_copy_paste() {
    let (tmp, photo) = photo_png_fixture();
    let mut harness = build_harness();
    harness.state_mut().set_module(Module::Library);
    open_file_and_restore_fixture(&mut harness, &photo, ready_rendered());
    assert_no_tmp_leak(&mut harness, tmp.path());
    harness
        .state_mut()
        .set_metadata_buffer("title", "Startschuss".to_string())
        .expect("set title buffer");
    harness
        .state_mut()
        .set_metadata_buffer("city", "Berlin".to_string())
        .expect("set city buffer");
    assert!(
        harness
            .state_mut()
            .commit_metadata_draft()
            .expect("commit metadata draft"),
        "entered metadata must persist (sidecar write)"
    );
    // Layout frame + expand the draft editor (there is no library_metadata
    // helper that opens it after a real-path load).
    harness.run();
    harness.run();
    let opened = harness
        .query_all_by_label("Metadata draft")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(opened, "Metadata draft section not found in headed harness");
    harness.run();
    // Copy through the panel's own copy/paste system.
    let copied = harness
        .query_all_by_label("Copy metadata")
        .next()
        .map(|node| {
            node.click();
            true
        })
        .unwrap_or(false);
    assert!(copied, "Copy metadata button not found in headed harness");
    harness.run();
    // Non-vacuous guards: the clipboard captured both entered fields and the
    // status proves the copy ran.
    assert!(
        harness.state_mut().status().contains("Metadata copied"),
        "copy must announce the copied fields, got {:?}",
        harness.state_mut().status()
    );
    harness.run();
    // Both clipboard buttons sit in the draft editor's action row, on-screen
    // without scrolling (scrolling would clip the panel's "Metadata" heading).
    assert_label_on_screen(&mut harness, "Copy metadata");
    assert_label_on_screen(&mut harness, "Paste metadata");
    assert_label_on_screen(&mut harness, "Title");
    harness.snapshot("library_metadata_copy_paste");
}
