//! LRPAR-G09-SORT-09: Library sort-order tests.
//!
//! Modes (Name / Capture Date / Custom), the portable `lumina-sort.json`
//! roundtrip, real grid drag-drop → Custom, the shared Grid/Filmstrip order
//! and the stack-as-unit guarantee. Fixtures are stub `*.cr3` files with a
//! valid sidecar so the RAW-only Library order lists them without a decode;
//! capture timestamps are injected directly (the stub bytes carry no EXIF).

use super::*;

// Rework B1/B2/B4: rejection-classes, sidecar-safety and drag-as-unit coverage
// (split out so both files stay within the 500-line ratchet).
mod rejections;

fn stub_identity(name: &str) -> SourceIdentity {
    SourceIdentity {
        relative_name: name.into(),
        content_hash: "blake3:stub".into(),
        byte_length: 0,
        modified_at: None,
        raw_format: "CR3".into(),
        orientation: 1,
        decode_fingerprint: DecodeFingerprint {
            decoder: "libraw".into(),
            version: "1".into(),
            parameters: BTreeMap::new(),
            extras: BTreeMap::new(),
        },
        geometry_fingerprint: GeometryFingerprint {
            width: 100,
            height: 100,
            orientation: 1,
            pixel_aspect_ratio: 1.0,
            extras: BTreeMap::new(),
        },
        extras: BTreeMap::new(),
    }
}

/// A stub RAW source with a valid (empty) sidecar.
fn stub_raw(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("stub:{name}").as_bytes()).unwrap();
    let document = SidecarDocument::new(stub_identity(name), "raster-mvp-1");
    lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&path), &document).unwrap();
    path
}

/// Scan `dir` without starting the background auto-load decode.
fn scan(app: &mut LuminaApp, dir: &Path) {
    app.auto_load_attempted = true;
    app.directory = dir.display().to_string();
    app.list_directory_flat();
}

/// Visible RAW names in display order.
fn visible_names(app: &LuminaApp) -> Vec<String> {
    app.raw_entry_indices()
        .iter()
        .map(|&index| app.entries[index].name.clone())
        .collect()
}

fn put_selection(app: &mut LuminaApp, paths: &[&Path]) {
    app.filmstrip_selection = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
}

fn sort_file(dir: &Path) -> PathBuf {
    dir.join("lumina-sort.json")
}

/// Default mode is `Name`; the RAW grid order is lexicographic.
#[test]
fn sort_defaults_to_name() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "c.cr3");
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert_eq!(visible_names(&app), vec!["a.cr3", "b.cr3", "c.cr3"]);
    // No sort file is written by merely listing a folder.
    assert!(!sort_file(dir.path()).exists());
}

/// `CaptureDate` orders by the EXIF timestamp and puts unknown timestamps last.
#[test]
fn sort_capture_date_orders_by_timestamp_unknown_last() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    for entry in app.entries.iter_mut() {
        entry.capture_timestamp = match entry.name.as_str() {
            "a.cr3" => Some(300),
            "b.cr3" => Some(100),
            "c.cr3" => None,
            _ => unreachable!("unexpected entry"),
        };
    }
    app.set_library_sort(LibrarySort::CaptureDate).unwrap();
    assert_eq!(app.library_sort(), LibrarySort::CaptureDate);
    assert_eq!(visible_names(&app), vec!["b.cr3", "a.cr3", "c.cr3"]);
    // Mode is persisted portably (no absolute path in the folder file).
    let raw = std::fs::read_to_string(sort_file(dir.path())).unwrap();
    assert!(raw.contains("\"capture_date\""), "{raw}");
    assert!(!raw.contains(&dir.path().display().to_string()), "{raw}");
}

/// A custom reorder switches to `Custom`, writes relative names and survives a
/// reload (mode + order restored from `lumina-sort.json`).
#[test]
fn custom_reorder_persists_across_reload() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    app.reorder_library_entry(&a.display().to_string(), &c.display().to_string())
        .unwrap();
    assert_eq!(app.library_sort(), LibrarySort::Custom);
    assert_eq!(visible_names(&app), vec!["b.cr3", "a.cr3", "c.cr3"]);
    assert_eq!(
        app.library_sort_order().to_vec(),
        vec![
            "b.cr3".to_string(),
            "a.cr3".to_string(),
            "c.cr3".to_string()
        ]
    );

    let raw = std::fs::read_to_string(sort_file(dir.path())).unwrap();
    assert!(raw.contains("\"custom\""), "{raw}");
    assert!(raw.contains("\"a.cr3\""), "{raw}");
    assert!(
        !raw.contains(&dir.path().display().to_string()),
        "portable order must not persist absolute paths: {raw}"
    );

    let mut reopened = new_app();
    scan(&mut reopened, dir.path());
    assert_eq!(
        reopened.library_sort(),
        LibrarySort::Custom,
        "the folder mode is restored"
    );
    assert_eq!(visible_names(&reopened), vec!["b.cr3", "a.cr3", "c.cr3"]);
}

/// Grid and filmstrip share the same display order in every mode.
#[test]
fn sort_applies_to_grid_and_filmstrip() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    app.reorder_library_entry(&c.display().to_string(), &a.display().to_string())
        .unwrap();
    let grid: Vec<String> = app
        .filtered_library_order()
        .iter()
        .map(|&index| app.entries[index].name.clone())
        .collect();
    let filmstrip: Vec<String> = app
        .filmstrip_order()
        .iter()
        .map(|path| {
            Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(grid, vec!["c.cr3", "a.cr3", "b.cr3"]);
    assert_eq!(filmstrip, grid);
}

/// A corrupt folder file is rejected loudly and falls back to `Name` (visible
/// status), never silently ignored.
#[test]
fn corrupt_sort_file_falls_back_loudly_to_name() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    std::fs::write(sort_file(dir.path()), b"{ not json").unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert_eq!(visible_names(&app), vec!["a.cr3", "b.cr3"]);
    assert!(
        app.status().contains("invalid sort order"),
        "corrupt file must surface visibly, got: {}",
        app.status()
    );
}

/// An absolute/unsafe custom entry is refused loudly (portability rule).
#[test]
fn sort_file_with_absolute_path_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    std::fs::write(
        sort_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["/etc/passwd"]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert!(
        app.status().contains("non-portable"),
        "absolute order entry must be refused, got: {}",
        app.status()
    );
}

/// A missing entry in the custom order is appended deterministically (never a
/// lost image).
#[test]
fn custom_order_appends_entries_missing_from_the_file() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    stub_raw(dir.path(), "c.cr3");
    std::fs::write(
        sort_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"custom","order":["c.cr3"]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Custom);
    // `c` first (explicit custom order), then the two unknown entries by key.
    assert_eq!(visible_names(&app), vec!["c.cr3", "a.cr3", "b.cr3"]);
}

/// The sort-mode buttons in the `\` drawer are clickable and switch the mode.
#[test]
fn sort_buttons_are_clickable_in_the_drawer() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    app.toggle_filter_bar();
    let _ = headless_click_label(&mut app, Str::LibrarySortDate.t(), |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_eq!(app.library_sort(), LibrarySort::CaptureDate);
    let _ = headless_click_label(&mut app, Str::LibrarySortCustom.t(), |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_eq!(app.library_sort(), LibrarySort::Custom);
    let _ = headless_click_label(&mut app, Str::LibrarySortName.t(), |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_eq!(app.library_sort(), LibrarySort::Name);
}

/// The custom order references subfolder images by their portable relative
/// name (`sub/file.ext`), never an absolute path, and reload restores it.
#[test]
fn custom_order_across_subfolders_stays_portable() {
    let dir = tempfile::tempdir().unwrap();
    let sub = dir.path().join("sub");
    std::fs::create_dir(&sub).unwrap();
    let top = stub_raw(dir.path(), "top.cr3");
    let nested = stub_raw(&sub, "nested.cr3");
    let mut app = new_app();
    app.auto_load_attempted = true;
    app.directory = dir.path().display().to_string();
    app.list_directory(); // recursive aggregation
    assert_eq!(visible_names(&app), vec!["nested.cr3", "top.cr3"]);
    app.reorder_library_entry(&top.display().to_string(), &nested.display().to_string())
        .unwrap();
    assert_eq!(
        app.library_sort_order().to_vec(),
        vec!["top.cr3".to_string(), "sub/nested.cr3".to_string()]
    );
    let raw = std::fs::read_to_string(sort_file(dir.path())).unwrap();
    assert!(raw.contains("sub/nested.cr3"), "{raw}");
    assert!(!raw.contains(&dir.path().display().to_string()), "{raw}");

    let mut reopened = new_app();
    reopened.auto_load_attempted = true;
    reopened.directory = dir.path().display().to_string();
    reopened.list_directory();
    assert_eq!(visible_names(&reopened), vec!["top.cr3", "nested.cr3"]);
}

/// The EXIF capture timestamp is actually read from a real RAW fixture (not
/// only injected): the committed aircraft CR3s carry DateTimeOriginal. This
/// pins the EXIF→`capture_timestamp` extraction the `CaptureDate` sort needs.
#[test]
fn capture_timestamp_is_populated_from_real_exif() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sample-data/raw");
    assert!(
        fixture.is_dir(),
        "committed RAW fixture dir missing: {}",
        fixture.display()
    );
    let mut app = new_app();
    app.auto_load_attempted = true;
    app.directory = fixture.display().to_string();
    app.list_directory_flat();
    let stamps: Vec<Option<i64>> = app
        .entries
        .iter()
        .filter(|entry| entry.name.ends_with(".cr3"))
        .map(|entry| entry.capture_timestamp)
        .collect();
    assert!(
        !stamps.is_empty(),
        "the committed RAW fixture must be listed"
    );
    assert!(
        stamps.iter().any(Option::is_some),
        "the EXIF capture timestamp must be read for the fixture: {stamps:?}"
    );
}

/// One headless grid frame (persistent context drives a real drag).
fn grid_pass(ctx: &egui::Context, app: &mut LuminaApp, time: &mut f64, events: Vec<egui::Event>) {
    *time += 1.0 / 60.0;
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(*time),
            events,
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            app.draw_library_grid(&ctx, ui);
        },
    );
    output.textures_delta.clear();
}

fn pointer_button(pos: egui::Pos2, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    }
}

/// Real grid drag-drop: dragging `a` onto `c` switches to `Custom` and writes
/// the folder file.
#[test]
fn grid_drag_drop_reorders_and_switches_to_custom() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    // Resolve the stable cell ids before the drag (path-keyed, order-independent).
    let key_of = |app: &LuminaApp, name: &str| -> String {
        app.entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .thumb_key
            .clone()
    };
    let key_a = key_of(&app, "a.cr3");
    let key_c = key_of(&app, "c.cr3");

    let ctx = egui::Context::default();
    let mut time = 0.0;
    grid_pass(&ctx, &mut app, &mut time, vec![]);
    let cell = |key: &str| -> egui::Rect {
        ctx.read_response(crate::library_sort::library_cell_id(key))
            .expect("the grid cell must be registered")
            .rect
    };
    let from = cell(&key_a).center();
    let to = cell(&key_c).center();
    let mid = from + (to - from) * 0.5;
    grid_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(from)],
    );
    grid_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
    );
    grid_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(mid)],
    );
    grid_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(to)],
    );
    grid_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(to), pointer_button(to, false)],
    );
    grid_pass(&ctx, &mut app, &mut time, vec![]);

    assert_eq!(
        app.library_sort(),
        LibrarySort::Custom,
        "a drag reorder must switch to Custom"
    );
    assert_eq!(visible_names(&app), vec!["b.cr3", "a.cr3", "c.cr3"]);
    assert!(
        sort_file(dir.path()).is_file(),
        "the custom order must be written to the folder file"
    );
}

/// A collapsed stack stays one sorted unit: sorting by capture date hides the
/// non-cover member; a drag on the cover moves the whole unit.
#[test]
fn collapsed_stack_stays_a_sorted_unit() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    let b = stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    app.toggle_stack_collapse().unwrap();
    for entry in app.entries.iter_mut() {
        entry.capture_timestamp = match entry.name.as_str() {
            "a.cr3" => Some(200),
            "b.cr3" => Some(100),
            "c.cr3" => Some(150),
            _ => unreachable!("unexpected entry"),
        };
    }
    app.set_library_sort(LibrarySort::CaptureDate).unwrap();
    assert_eq!(
        visible_names(&app),
        vec!["c.cr3", "a.cr3"],
        "a collapsed stack contributes only its cover"
    );

    // A drag on the cover moves the whole stack as one unit.
    app.reorder_library_entry(&c.display().to_string(), &a.display().to_string())
        .unwrap();
    let order = app.library_sort_order().to_vec();
    let a_at = order.iter().position(|key| key == "a.cr3").unwrap();
    let b_at = order.iter().position(|key| key == "b.cr3").unwrap();
    assert_eq!(
        (a_at as isize - b_at as isize).abs(),
        1,
        "stack members must stay contiguous in the custom order: {order:?}"
    );
    assert_eq!(visible_names(&app), vec!["c.cr3", "a.cr3"]);

    // Expanding reveals every member again — the membership is intact.
    app.toggle_stack_collapse().unwrap();
    let expanded = visible_names(&app);
    assert_eq!(expanded.len(), 3, "expanded stack lists all members");
    assert!(expanded.contains(&"b.cr3".to_string()));
}
