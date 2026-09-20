//! LRPAR-G09-SORT-09 (Rework B1/B2/B4): rejection-classes, sidecar-safety and
//! drag-as-unit coverage.
//!
//! Split out of `tests/library_sort.rs` (module `tests::library_sort`) so both
//! files stay within the 500-line ratchet; the shared fixtures/helpers live in
//! the parent module and are pulled in via `use super::*`.

use super::*;

/// B1(a): a higher `version` (=2) is forward-incompatible and rejected loudly
/// (the valid custom order in the file must NOT be applied silently).
#[test]
fn sort_file_with_higher_version_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    std::fs::write(
        sort_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":2,"mode":"custom","order":["b.cr3","a.cr3"]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert_eq!(visible_names(&app), vec!["a.cr3", "b.cr3"]);
    assert!(
        app.status().contains("unsupported sort-order version"),
        "a higher version must surface loudly, got: {}",
        app.status()
    );
}

/// B1(b): an unknown `format` marker is rejected loudly (never parsed as a
/// valid Lumina sort file).
#[test]
fn sort_file_with_unknown_format_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    std::fs::write(
        sort_file(dir.path()),
        br#"{"format":"some-other-tool","version":1,"mode":"custom","order":["b.cr3","a.cr3"]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert_eq!(visible_names(&app), vec!["a.cr3", "b.cr3"]);
    assert!(
        app.status().contains("unexpected sort-order format"),
        "an unknown format must surface loudly, got: {}",
        app.status()
    );
}

/// B1(c): an unknown `mode` (valid format/version/order) is rejected loudly;
/// `library_sort.rs` never falls back silently to a default mode.
#[test]
fn sort_file_with_unknown_mode_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    std::fs::write(
        sort_file(dir.path()),
        br#"{"format":"lumina-folder-sort","version":1,"mode":"shuffle","order":[]}"#,
    )
    .unwrap();
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert_eq!(app.library_sort(), LibrarySort::Name);
    assert_eq!(visible_names(&app), vec!["a.cr3", "b.cr3"]);
    assert!(
        app.status().contains("unknown sort mode"),
        "an unknown mode must surface loudly, got: {}",
        app.status()
    );
}

/// B2: sorting and drag-drop reordering are display-only — sidecar and original
/// bytes stay identical (SOLL: „nie Rezept/Sidecar").
#[test]
fn sort_actions_leave_sidecar_and_original_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    let b = stub_raw(dir.path(), "b.cr3");
    let sidecar_a = lumina_sidecar::sidecar_path_for(&a);
    let sidecar_b = lumina_sidecar::sidecar_path_for(&b);
    let before_original_a = std::fs::read(&a).unwrap();
    let before_sidecar_a = std::fs::read(&sidecar_a).unwrap();
    let before_sidecar_b = std::fs::read(&sidecar_b).unwrap();

    let mut app = new_app();
    scan(&mut app, dir.path());
    app.set_library_sort(LibrarySort::Custom).unwrap();
    app.reorder_library_entry(&b.display().to_string(), &a.display().to_string())
        .unwrap();
    app.set_library_sort(LibrarySort::CaptureDate).unwrap();

    assert_eq!(
        std::fs::read(&a).unwrap(),
        before_original_a,
        "sorting must never touch the original"
    );
    assert_eq!(
        std::fs::read(&sidecar_a).unwrap(),
        before_sidecar_a,
        "sorting must never touch the sidecar"
    );
    assert_eq!(std::fs::read(&sidecar_b).unwrap(), before_sidecar_b);
    assert!(
        sort_file(dir.path()).is_file(),
        "only the folder sort file is written"
    );
}

/// B4: a real pointer drag of a stack member moves the whole stack as one unit
/// (the drag payload is the member path; the unit expands in the handler).
#[test]
fn grid_drag_of_a_stack_member_moves_the_whole_unit() {
    let dir = tempfile::tempdir().unwrap();
    let a = stub_raw(dir.path(), "a.cr3");
    let b = stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    stub_raw(dir.path(), "d.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    // Stack {b, c} (b is the cover), expanded so both members paint.
    app.path = b.display().to_string();
    put_selection(&mut app, &[&b, &c]);
    app.create_stack_from_selection().unwrap();

    let key_of = |app: &LuminaApp, name: &str| -> String {
        app.entries
            .iter()
            .find(|entry| entry.name == name)
            .unwrap()
            .thumb_key
            .clone()
    };
    let key_b = key_of(&app, "b.cr3");
    let key_a = key_of(&app, "a.cr3");

    let ctx = egui::Context::default();
    let mut time = 0.0;
    grid_pass(&ctx, &mut app, &mut time, vec![]);
    let from = ctx
        .read_response(crate::library_sort::library_cell_id(&key_b))
        .expect("stack member cell registered")
        .rect
        .center();
    let to = ctx
        .read_response(crate::library_sort::library_cell_id(&key_a))
        .expect("target cell registered")
        .rect
        .center();
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

    assert_eq!(app.library_sort(), LibrarySort::Custom);
    assert_eq!(
        visible_names(&app),
        vec!["b.cr3", "c.cr3", "a.cr3", "d.cr3"],
        "dragging one member must move the whole stack unit before the target"
    );
    let order = app.library_sort_order().to_vec();
    let b_at = order.iter().position(|key| key == "b.cr3").unwrap();
    let c_at = order.iter().position(|key| key == "c.cr3").unwrap();
    assert_eq!(
        (b_at as isize - c_at as isize).abs(),
        1,
        "the unit stays contiguous: {order:?}"
    );
}
