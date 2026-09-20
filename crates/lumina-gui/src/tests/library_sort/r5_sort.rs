//! R5-SORT-1 (F-103-N6 Runde 5, User-Bug): the Library sort modes must be
//! findable without knowing the `\` shortcut (an always-visible sort row in the
//! grid header) and a custom drag must show an insertion line at the drop
//! target. Nested under `tests::library_sort` so the 500-line parent stays
//! unchanged.

use super::*;

/// Like the parent `grid_pass`, but returns the painted shapes so the drop
/// indicator can be inspected.
fn grid_shapes_pass(
    ctx: &egui::Context,
    app: &mut LuminaApp,
    time: &mut f64,
    events: Vec<egui::Event>,
) -> Vec<egui::epaint::ClippedShape> {
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
    output.shapes
}

/// R5-SORT-1: the sort row is painted with the drawer closed and its buttons
/// really switch the mode.
#[test]
fn sort_row_is_visible_and_clickable_without_the_filter_drawer() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
    assert!(
        !app.filter_bar_visible,
        "precondition: the `\\` drawer is hidden"
    );

    let shapes = headless_shapes(&mut app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    for label in [
        Str::LibrarySortName.t(),
        Str::LibrarySortDate.t(),
        Str::LibrarySortCustom.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }

    let _ = headless_click_label(&mut app, Str::LibrarySortCustom.t(), |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_eq!(
        app.library_sort(),
        LibrarySort::Custom,
        "the visible Custom button must switch the sort mode"
    );
}

/// R5-SORT-1: while a grid cell is dragged over another cell, an insertion line
/// is painted at the drop target (insert-before) so the drop position is
/// visible before the release.
#[test]
fn custom_drag_paints_an_insertion_line_at_the_drop_target() {
    let dir = tempfile::tempdir().unwrap();
    stub_raw(dir.path(), "a.cr3");
    stub_raw(dir.path(), "b.cr3");
    let c = stub_raw(dir.path(), "c.cr3");
    let mut app = new_app();
    scan(&mut app, dir.path());
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
    let _ = c;

    let ctx = egui::Context::default();
    let mut time = 0.0;
    grid_shapes_pass(&ctx, &mut app, &mut time, vec![]);
    let cell = |key: &str| -> egui::Rect {
        ctx.read_response(crate::library_sort::library_cell_id(key))
            .expect("the grid cell must be registered")
            .rect
    };
    let from = cell(&key_a).center();
    let to = cell(&key_c).center();
    grid_shapes_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(from)],
    );
    grid_shapes_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(from), pointer_button(from, true)],
    );
    grid_shapes_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(to)],
    );

    let shapes = grid_shapes_pass(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(to)],
    );
    let has_indicator = shapes.iter().any(|clipped| match &clipped.shape {
        egui::Shape::LineSegment { stroke, .. } => stroke.color == crate::theme::ACCENT,
        _ => false,
    });
    assert!(
        has_indicator,
        "a custom drag must paint the insertion line at the hovered cell"
    );
}
