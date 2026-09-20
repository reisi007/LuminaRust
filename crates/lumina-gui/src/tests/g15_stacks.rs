//! LRPAR-G15-STACK-15: image-stack GUI tests (Sidecar-first persistence,
//! Grid/Filmstrip collapse, stack-as-unit selection, create/unstack/toggle).
//!
//! Every test drives the shared LuminaApp mutators (no GUI-only shortcut);
//! the display assertions paint the real Grid/Filmstrip headless. The fixture
//! sources are stub `*.cr3` files with a valid sidecar so the RAW-only Library
//! order includes them without a decode.

use super::*;

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

/// A stub RAW source with a valid (empty) sidecar, so a scan lists it in the
/// RAW-only Library order and the stack section can round-trip.
fn stub_raw(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("stub:{name}").as_bytes()).unwrap();
    let document = SidecarDocument::new(stub_identity(name), "raster-mvp-1");
    lumina_sidecar::save_sidecar(&lumina_sidecar::sidecar_path_for(&path), &document).unwrap();
    path
}

/// A stub RAW source without a sidecar (used for the missing-sidecar case).
fn stub_raw_no_sidecar(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("stub:{name}").as_bytes()).unwrap();
    path
}

/// Scan `dir` without starting the background auto-load decode (the stub
/// `.cr3` files are not decodable and the tests never open them).
fn scan(app: &mut LuminaApp, dir: &Path) {
    app.auto_load_attempted = true;
    app.directory = dir.display().to_string();
    app.list_directory_flat();
}

fn stack_section(path: &Path) -> Option<lumina_sidecar::StackMembership> {
    lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(path))
        .unwrap()
        .stack
}

fn put_selection(app: &mut LuminaApp, paths: &[&Path]) {
    app.filmstrip_selection = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
}

/// Grid paint pass over the current listing.
fn grid_shapes(app: &mut LuminaApp) -> Vec<egui::epaint::ClippedShape> {
    headless_shapes(app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    })
}

/// Filmstrip paint pass over the current listing.
fn filmstrip_shapes(app: &mut LuminaApp) -> Vec<egui::epaint::ClippedShape> {
    headless_shapes(app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_filmstrip(&ctx, ui);
    })
}

/// E2E (DoD §1): create → sidecar files → reload in a fresh app restores the
/// stack (membership + collapse state); the original files stay untouched.
#[test]
fn g15_stack_create_collapse_reload() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let original_a = std::fs::read(&a).unwrap();

    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();

    for path in [&a, &b] {
        let stack = stack_section(path).expect("both members carry the stack section");
        assert_eq!(
            stack.members,
            vec!["a.cr3".to_string(), "b.cr3".to_string()]
        );
        assert_eq!(stack.cover, "a.cr3");
        assert!(!stack.collapsed);
    }

    // Collapse and persist.
    app.toggle_stack_collapse().unwrap();
    for path in [&a, &b] {
        let stack = stack_section(path).unwrap();
        assert!(
            stack.collapsed,
            "collapse must persist in every member sidecar"
        );
    }

    // Reload in a fresh app: the collapsed stack is restored.
    let mut reopened = new_app();
    scan(&mut reopened, directory.path());
    let restored = reopened.stack_for_path(&a.display().to_string()).unwrap();
    assert!(restored.collapsed);
    assert_eq!(restored.members.len(), 2);
    assert_eq!(
        reopened.active_stack_collapsed(),
        None,
        "no image loaded yet"
    );
    reopened.path = a.display().to_string();
    assert_eq!(reopened.active_stack_collapsed(), Some(true));

    // Original bytes are never touched by a stack operation.
    assert_eq!(std::fs::read(&a).unwrap(), original_a);
}

/// Collapsed Grid/Filmstrip show only the cover (with the badge); expanding
/// reveals every member. The listing order itself is the assertion anchor.
#[test]
fn g15_stack_grid_and_filmstrip_collapse_display() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    app.toggle_stack_collapse().unwrap();

    let names = |app: &LuminaApp| -> Vec<String> {
        app.raw_entry_indices()
            .iter()
            .map(|&index| app.entries[index].name.clone())
            .collect()
    };
    assert_eq!(names(&app), vec!["a.cr3".to_string()], "collapsed hides b");

    let shapes = grid_shapes(&mut app);
    assert!(
        text_contains(&shapes, "a.cr3"),
        "cover must paint in the collapsed grid"
    );
    assert!(
        !text_contains(&shapes, "b.cr3"),
        "hidden member must not paint in the collapsed grid"
    );
    assert!(
        text_contains(&shapes, "⊞ 2"),
        "collapsed cover paints the stack badge"
    );

    let shapes = filmstrip_shapes(&mut app);
    assert!(text_contains(&shapes, "a.cr3"));
    assert!(!text_contains(&shapes, "b.cr3"));
    assert!(text_contains(&shapes, "⊞ 2"));

    // Expand: both entries are listed and painted again.
    app.toggle_stack_collapse().unwrap();
    assert_eq!(
        names(&app),
        vec!["a.cr3".to_string(), "b.cr3".to_string()],
        "expanded shows every member"
    );
    let shapes = grid_shapes(&mut app);
    assert!(text_contains(&shapes, "a.cr3") && text_contains(&shapes, "b.cr3"));
    assert!(text_contains(&shapes, "⊟ 2"));
}

/// Selecting one member selects the whole stack, so Sync/Batch act on the
/// unit; toggling the stack off removes every member.
#[test]
fn g15_stack_selects_as_unit_for_batch() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    app.toggle_stack_collapse().unwrap();

    // Click the collapsed cover: both members end up selected.
    app.filmstrip_selection.clear();
    app.select_filmstrip_path(a.display().to_string(), false, false);
    let selected = app.filmstrip_selection();
    assert!(selected.contains(&a.display().to_string()));
    assert!(selected.contains(&b.display().to_string()));

    // Batch over the selection reaches both sidecars.
    let op = parse_metadata_batch_op("add_keyword", "stacked").unwrap();
    let report = app.apply_metadata_batch(&op);
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
    for path in [&a, &b] {
        let document =
            lumina_sidecar::load_sidecar(&lumina_sidecar::sidecar_path_for(path)).unwrap();
        assert!(document.keywords.contains(&"stacked".to_string()));
    }

    // Toggle the cover off: the whole unit leaves the selection.
    app.select_filmstrip_path(a.display().to_string(), true, false);
    assert!(app.filmstrip_selection().is_empty());
}

/// Sync Settings reaches every member of the unit selection produced by a
/// stack click (real decodable PNG fixtures, so the recipe path runs fully).
#[test]
fn g15_stack_sync_reaches_all_members() {
    let directory = tempfile::tempdir().unwrap();
    let a = directory.path().join("a.png");
    let b = directory.path().join("b.png");
    save_png(&a);
    save_png(&b);
    for source in [&a, &b] {
        let mut setup = new_app();
        open_and_decode(&mut setup, source.display().to_string());
        setup.add_keyword("seed").unwrap();
    }
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();

    // The unit selection a stack click produces: expanding the cover reaches
    // the second member (shared `LuminaApp` helper used by `select_filmstrip_path`).
    let unit = app.expand_selection_to_stacks(&BTreeSet::from([a.display().to_string()]));
    app.filmstrip_selection = unit;
    assert!(app.filmstrip_selection.contains(&b.display().to_string()));

    let report = app.sync_settings_to_selection();
    assert_eq!(report.applied_count(), 2);
    assert_eq!(report.failed_count(), 0);
}

/// Collapsed navigation treats the stack as one entry: moving from the cover
/// skips the hidden member.
#[test]
fn g15_stack_navigation_skips_hidden_members() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let c = stub_raw(directory.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    app.toggle_stack_collapse().unwrap();

    let moved = app.move_library_selection(1).unwrap();
    assert_eq!(
        moved,
        c.display().to_string(),
        "b is hidden behind the cover"
    );
    let moved = app.move_library_selection(-1).unwrap();
    assert_eq!(moved, a.display().to_string());
}

/// Unstack clears the membership in every member sidecar.
#[test]
fn g15_stack_unstack_dissolves_membership() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    assert!(stack_section(&a).is_some());

    app.unstack_selection().unwrap();
    assert!(stack_section(&a).is_none());
    assert!(stack_section(&b).is_none());
}

/// Loud refusals: fewer than two images, a foreign folder and a missing
/// sidecar all fail without writing anything.
#[test]
fn g15_stack_create_refusals_are_loud() {
    let directory = tempfile::tempdir().unwrap();
    let other = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let foreign = stub_raw(other.path(), "f.cr3");
    let no_sidecar = stub_raw_no_sidecar(directory.path(), "n.cr3");

    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();

    put_selection(&mut app, &[&a]);
    assert!(
        app.create_stack_from_selection().is_err(),
        "one image is not a stack"
    );

    put_selection(&mut app, &[&a, &foreign]);
    assert!(
        app.create_stack_from_selection().is_err(),
        "cross-folder stack refused"
    );

    put_selection(&mut app, &[&a, &no_sidecar]);
    assert!(
        app.create_stack_from_selection().is_err(),
        "missing sidecar refused"
    );

    assert!(
        stack_section(&a).is_none(),
        "no partial stack may be written"
    );
    assert!(stack_section(&b).is_none());
}

/// A stack whose cover was deleted keeps showing its present members instead
/// of vanishing silently (the collapse only applies while the cover is listed).
#[test]
fn g15_stack_missing_cover_keeps_members_visible() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    app.toggle_stack_collapse().unwrap();
    assert_eq!(app.raw_entry_indices().len(), 1, "collapsed to the cover");

    // Remove the cover source and its sidecar entirely.
    std::fs::remove_file(&a).unwrap();
    std::fs::remove_file(lumina_sidecar::sidecar_path_for(&a)).unwrap();
    let mut reopened = new_app();
    scan(&mut reopened, directory.path());
    let names: Vec<String> = reopened
        .raw_entry_indices()
        .iter()
        .map(|&index| reopened.entries[index].name.clone())
        .collect();
    assert_eq!(names, vec!["b.cr3".to_string()], "b must stay visible");
}

/// The Library metadata panel exposes clickable Stack/Unstack buttons that
/// drive the same mutators (F-100 clickability, button-first path).
#[test]
fn g15_stack_panel_buttons_are_clickable() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);

    let _ = headless_click_label(&mut app, Str::StackGroup.t(), |app, ui| {
        app.draw_library_metadata(ui)
    });
    assert!(
        stack_section(&a).is_some(),
        "the Stack button must create the stack"
    );

    let _ = headless_click_label(&mut app, Str::StackUngroup.t(), |app, ui| {
        app.draw_library_metadata(ui)
    });
    assert!(
        stack_section(&a).is_none(),
        "the Unstack button must dissolve it"
    );
}

/// B-2: the panel collapse/expand toggle is a real clickable button and the
/// status label reflects the persisted state after each click.
#[test]
fn g15_stack_panel_toggle_button_is_clickable() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();
    assert_eq!(app.stack_status_label(), "2 image(s), expanded");

    // Expanded -> the toggle offers collapse ("⊟").
    let _ = headless_click_label(&mut app, "⊟", |app, ui| app.draw_library_metadata(ui));
    assert!(
        stack_section(&a).unwrap().collapsed,
        "the toggle must persist the collapse"
    );
    assert_eq!(app.stack_status_label(), "2 image(s), collapsed");

    // Collapsed -> the toggle offers expand ("⊞").
    let _ = headless_click_label(&mut app, "⊞", |app, ui| app.draw_library_metadata(ui));
    assert!(
        !stack_section(&a).unwrap().collapsed,
        "the toggle must persist the expand"
    );
    assert_eq!(app.stack_status_label(), "2 image(s), expanded");
}

/// The painted stack badge is clickable and toggles the collapse of its stack
/// (not of the loaded image).
#[test]
fn g15_stack_badge_click_toggles_collapse() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();

    let thumb_key = app
        .entries()
        .iter()
        .find(|entry| entry.name == "a.cr3")
        .unwrap()
        .thumb_key()
        .to_string();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1400.0, 900.0));
    let mut time = 0.0_f64;
    let mut run = |app: &mut LuminaApp, events: Vec<egui::Event>| {
        time += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let ctx = ui.ctx().clone();
                app.draw_library_grid(&ctx, ui);
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    let _ = run(&mut app, vec![]);
    let badge = ctx
        .read_response(crate::library_stacks::stack_badge_id(&thumb_key))
        .expect("the cover badge must be registered")
        .rect;
    let pos = badge.center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = run(&mut app, vec![egui::Event::PointerMoved(pos), click(true)]);
    let _ = run(&mut app, vec![egui::Event::PointerMoved(pos), click(false)]);
    let _ = run(&mut app, vec![]);

    assert!(
        stack_section(&a).unwrap().collapsed,
        "clicking the badge collapses the stack"
    );
}
