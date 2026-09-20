//! R5-STACK-2 (F-103-N6 Runde 5, User-Bug): the per-stack badge at a grid cell
//! must toggle that stack **visibly** (hide/show its non-cover members), not
//! only write the sidecar. Nested so the 500-line parent stays unchanged.

use super::*;

/// One grid paint pass in a persistent context.
fn grid_run(
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

fn visible_names(app: &LuminaApp) -> Vec<String> {
    app.raw_entry_indices()
        .iter()
        .map(|&index| app.entries[index].name.clone())
        .collect()
}

/// R5-STACK-2: a **non-cover** member's badge toggles the same stack — that is
/// the exact cell the user clicked in the run (the cover was elsewhere).
#[test]
fn non_cover_badge_click_visibly_collapses_the_grid() {
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
        .find(|entry| entry.name == "b.cr3")
        .unwrap()
        .thumb_key()
        .to_string();
    let ctx = egui::Context::default();
    let mut time = 0.0_f64;
    let badge_id = crate::library_stacks::stack_badge_id(
        crate::library_stacks::StackBadgeSurface::Grid,
        &thumb_key,
    );

    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);
    assert!(text_contains(&shapes, "b.cr3"));
    let pos = ctx
        .read_response(badge_id)
        .expect("the member badge must be registered")
        .rect
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(true)],
    );
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(false)],
    );
    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);

    assert_eq!(
        visible_names(&app),
        vec!["a.cr3".to_string()],
        "a non-cover badge click must visibly collapse the stack"
    );
    assert!(!text_contains(&shapes, "b.cr3"));
}

/// R5-STACK-2 root cause: the grid and the filmstrip are drawn in the same
/// frame (bottom panel first, then the central grid) and both register the
/// stack badge under the SAME `stack_badge_id(thumb_key)`. egui keys widget
/// interaction by id, so the second registration overwrites the first — the
/// badge of one surface becomes unclickable. This mirrors the app frame and
/// clicks the **grid** badge position.
#[test]
fn grid_badge_is_clickable_with_the_filmstrip_drawn_too() {
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
                egui::Panel::bottom("filmstrip").show(ui, |ui| app.draw_filmstrip(&ctx, ui));
                egui::CentralPanel::default().show(ui, |ui| {
                    app.draw_library_grid(&ctx, ui);
                });
            },
        );
        output.textures_delta.clear();
    };
    run(&mut app, vec![]);
    // The grid cell rect is uniquely identified; the badge sits top-centre.
    let cell = ctx
        .read_response(crate::library_sort::library_cell_id(&thumb_key))
        .expect("the grid cell must be registered")
        .rect;
    let pos = egui::pos2(cell.center().x, cell.top() + 10.0);
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(&mut app, vec![egui::Event::PointerMoved(pos), click(true)]);
    run(&mut app, vec![egui::Event::PointerMoved(pos), click(false)]);
    run(&mut app, vec![]);
    assert_eq!(
        visible_names(&app),
        vec!["a.cr3".to_string()],
        "the grid badge must be clickable although the filmstrip registered the same id"
    );
}

/// R5-STACK-3: the membership bracket and the "index/count" position are
/// painted for every stacked cell, so grouping is recognizable without a
/// selection; collapsed keeps the clear stack symbol.
#[test]
fn stack_membership_bracket_and_index_are_painted() {
    let directory = tempfile::tempdir().unwrap();
    let a = stub_raw(directory.path(), "a.cr3");
    let b = stub_raw(directory.path(), "b.cr3");
    let mut app = new_app();
    scan(&mut app, directory.path());
    app.path = a.display().to_string();
    put_selection(&mut app, &[&a, &b]);
    app.create_stack_from_selection().unwrap();

    let ctx = egui::Context::default();
    let mut time = 0.0_f64;
    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);
    // Cover shows its position "1/2", member "2/2".
    assert!(
        text_contains(&shapes, "⊟ 1/2") && text_contains(&shapes, "⊟ 2/2"),
        "both members must paint their stack position"
    );
    let group_color = egui::Color32::from_rgb(0, 190, 235);
    let frames = shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect) => rect.stroke.color == group_color,
            _ => false,
        })
        .count();
    assert_eq!(
        frames, 2,
        "each stacked cell must paint exactly one membership bracket"
    );
}

/// R5-STACK-2: the filmstrip's per-stack badge toggles the stack visibly too.
#[test]
fn filmstrip_badge_click_visibly_collapses() {
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
                app.draw_filmstrip(&ctx, ui);
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    run(&mut app, vec![]);
    let badge_id = crate::library_stacks::stack_badge_id(
        crate::library_stacks::StackBadgeSurface::Filmstrip,
        &thumb_key,
    );
    let pos = ctx
        .read_response(badge_id)
        .expect("the filmstrip cover badge must be registered")
        .rect
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(&mut app, vec![egui::Event::PointerMoved(pos), click(true)]);
    run(&mut app, vec![egui::Event::PointerMoved(pos), click(false)]);
    let shapes = run(&mut app, vec![]);
    assert_eq!(visible_names(&app), vec!["a.cr3".to_string()]);
    assert!(!text_contains(&shapes, "b.cr3"));
    assert!(text_contains(&shapes, "⊞ 1/2"));
}

/// R5-STACK-2: clicking the cover's per-stack badge collapses the stack
/// **visibly** — the non-cover member disappears from the grid and the badge
/// flips to the collapsed symbol; clicking again expands it.
#[test]
fn badge_click_visibly_collapses_and_expands_the_grid() {
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
    let mut time = 0.0_f64;
    let badge_id = crate::library_stacks::stack_badge_id(
        crate::library_stacks::StackBadgeSurface::Grid,
        &thumb_key,
    );

    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);
    assert!(text_contains(&shapes, "b.cr3"), "expanded stack shows both");
    let pos = ctx
        .read_response(badge_id)
        .expect("the cover badge must be registered")
        .rect
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(true)],
    );
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(false)],
    );
    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);

    assert_eq!(
        visible_names(&app),
        vec!["a.cr3".to_string()],
        "the badge click must visibly collapse the stack"
    );
    assert!(
        !text_contains(&shapes, "b.cr3"),
        "the collapsed member must not paint in the grid"
    );
    assert!(
        text_contains(&shapes, "⊞ 1/2"),
        "the collapsed cover must paint the collapsed badge"
    );

    // Click the (now collapsed) cover badge again: the member reappears.
    let pos = ctx
        .read_response(badge_id)
        .expect("the badge stays registered")
        .rect
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(true)],
    );
    let _ = grid_run(
        &ctx,
        &mut app,
        &mut time,
        vec![egui::Event::PointerMoved(pos), click(false)],
    );
    let shapes = grid_run(&ctx, &mut app, &mut time, vec![]);
    assert_eq!(
        visible_names(&app),
        vec!["a.cr3".to_string(), "b.cr3".to_string()],
        "the second badge click must expand the stack visibly"
    );
    assert!(text_contains(&shapes, "b.cr3"));
    assert!(text_contains(&shapes, "⊟ 1/2"));
}
