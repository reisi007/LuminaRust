//! The Library grid's sort controls under a real headless click: the sort-mode
//! buttons in the `\` drawer and the grid drag-drop reorder.
//!
//! Split out of `tests/library_sort.rs` (module `tests::library_sort`) to buy
//! headroom under the 500-line ratchet. The boundary is the F-100
//! clickability contract of the *sort widgets* — "does a real click on this
//! control change the order/mode?" — while the parent keeps the persisted-order
//! contract (modes, round-trips, portability, rejection classes). The two test
//! bodies moved verbatim; the shared fixtures/helpers and the drag harness
//! (`grid_pass`, `pointer_button`) stay in the parent module, which this file
//! pulls in via `use super::*`.

use super::*;

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
