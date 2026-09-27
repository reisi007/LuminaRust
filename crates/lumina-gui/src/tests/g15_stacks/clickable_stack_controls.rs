//! The clickable stack controls: the Library metadata panel's Stack/Unstack
//! buttons, its collapse/expand toggle, and the painted grid badge.
//!
//! Split out of `tests/g15_stacks.rs` (module `tests::g15_stacks`) to buy
//! headroom under the 500-line ratchet. The boundary is the F-100
//! clickability contract of the *stack controls* — "does a real headless click
//! on this control drive the shared mutator?" — while the parent keeps the
//! stack lifecycle/persistence contract (create, unstack, refusals, collapse
//! listing, selection-as-unit). The three test bodies moved verbatim and pull
//! the shared fixtures/helpers in via `use super::*`.

use super::*;

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
        .read_response(crate::library_stacks::stack_visuals::stack_badge_id(
            crate::library_stacks::StackBadgeSurface::Grid,
            &thumb_key,
        ))
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
