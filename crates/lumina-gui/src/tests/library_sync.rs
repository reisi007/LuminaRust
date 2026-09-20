//! library view selection sync and ready-probe badge tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// R3-GRIDSEL-1: the Library grid highlight follows the shared filmstrip
/// selection, never the loaded `self.path`. Reproduces the report exactly:
/// landscape loaded, portrait selected — the selected (not the loaded) cell
/// must carry the selection stroke.
#[test]
fn grid_highlight_follows_filmstrip_selection_not_loaded_path() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    // Same-directory fabricated entries: the RAW-only grid order holds both.
    app.directory = dir.path().display().to_string();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
    ];
    let order = app.filmstrip_order();
    assert_eq!(order.len(), 2, "both RAW entries are in the grid");
    // Loaded = a, selected = b (the bug report: grid showed the loaded cell).
    app.path = order[0].clone();
    app.filmstrip_selection = BTreeSet::from([order[1].clone()]);

    let mut selection_color: Option<egui::Color32> = None;

    let (shapes, _ctx) = headless_frame(&mut app, |app, ui| {
        selection_color = Some(ui.visuals().selection.bg_fill);
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    let selection_color = selection_color.expect("the grid paints with ui visuals");
    let highlights: Vec<egui::Rect> = shapes
        .iter()
        .filter_map(|clipped| match &clipped.shape {
            egui::Shape::Rect(rect)
                if rect.stroke.width == 2.0 && rect.stroke.color == selection_color =>
            {
                Some(rect.rect)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        highlights.len(),
        1,
        "exactly the selected cell is highlighted"
    );
    let b_center = text_shapes_for(&shapes, "b.cr3")
        .into_iter()
        .next()
        .expect("selected cell paints its filename")
        .0
        .center();
    let a_center = text_shapes_for(&shapes, "a.cr3")
        .into_iter()
        .next()
        .expect("loaded cell paints its filename")
        .0
        .center();
    assert!(
        highlights[0].contains(b_center),
        "the selected-but-unloaded cell must be highlighted"
    );
    assert!(
        !highlights[0].contains(a_center),
        "the loaded-but-unselected cell must NOT be highlighted"
    );
}

/// GUI-TOAST-OVERLAP-1: a `Ready` neighbor probe raises NO per-cell badge
/// (the transient overlay toast owns that signal) — the thumbnail cell
/// stays uncovered. Loading/Stale/Failed keep their small corner chips.
#[test]
fn ready_probe_shows_no_cell_badge() {
    use lumina_core::preview_cache::PreviewKind;
    use std::time::{Duration, Instant};

    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("neighbor.png");
    let (png, _) = synthetic_gradient_png();
    std::fs::write(&source, &png).unwrap();

    let (mut ctrl, _queue) = preview_ctrl::PreviewController::spawn(1);
    ctrl.enqueue(preview_ctrl::PreviewJob {
        probe_id: "neighbor-probe".into(),
        source: source.clone(),
        name: "neighbor.png".into(),
        virtual_copy: "vc-original".into(),
        target: (64, 64),
        kind: PreviewKind::Screen,
        priority: 0,
        denoise_policy: lumina_core::DenoisePolicy::Warn,
    });
    let deadline = Instant::now() + Duration::from_secs(10);
    while ctrl.probe_state("neighbor-probe") != preview_ctrl::PreviewProbeState::Ready
        && Instant::now() < deadline
    {
        ctrl.poll();
        std::thread::sleep(Duration::from_millis(20));
    }
    ctrl.poll();
    assert_eq!(
        ctrl.probe_state("neighbor-probe"),
        preview_ctrl::PreviewProbeState::Ready
    );
    let mut app = new_app();
    app.preview_ctrl = Some(ctrl);
    // Not the active image (the active image never shows a badge).
    app.preview_ctrl.as_mut().unwrap().set_active("other-probe");
    assert!(
        app.neighbor_preview_badge("neighbor-probe").is_none(),
        "a Ready probe must not cover its thumbnail cell with a badge"
    );
    assert!(
        app.neighbor_preview_badge("unknown-probe").is_none(),
        "a Miss probe shows no badge either"
    );
}

/// GUI-RIGHT-THUMB-1 + GUI-FILMSTRIP-DUP-1: every image appears exactly
/// once per view — the filmstrip order, the navigator rail and the
/// Library grid share one RAW index source with no duplicates.
#[test]
fn each_image_once_per_view_no_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
        raw_entry(dir.path(), "notes.png"),
        raw_entry(dir.path(), "c.cr3"),
    ];
    let indices = app.raw_entry_indices();
    assert_eq!(indices, vec![0, 1, 3], "RAW-only indices in display order");
    let order = app.filmstrip_order();
    assert_eq!(order.len(), 3);
    let mut sorted = order.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "filmstrip order must hold no duplicates");
    // The rail and the grid iterate the same index source.
    let rail: Vec<String> = indices
        .iter()
        .map(|&i| app.entries[i].path.display().to_string())
        .collect();
    assert_eq!(rail, order);
}

/// GUI-FILMSTRIP-DUP-1: selection syncs identically no matter which view
/// was clicked — filmstrip, navigator rail and Library grid all route
/// through the same bookkeeping (rail/grid call the shared helpers).
#[test]
fn selection_syncs_identically_from_every_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
        raw_entry(dir.path(), "c.cr3"),
    ];
    let order = app.filmstrip_order();
    // Plain click (filmstrip, rail, grid single-click): exactly the image.
    app.select_filmstrip_path(order[1].clone(), false, false);
    assert_eq!(app.filmstrip_selection(), vec![order[1].clone()]);
    // Toggle (Cmd/Ctrl-click): adds without opening.
    app.select_filmstrip_path(order[2].clone(), true, false);
    assert_eq!(
        app.filmstrip_selection(),
        vec![order[1].clone(), order[2].clone()]
    );
    // Range (Shift-click from the anchor): fills the span.
    app.select_filmstrip_path(order[0].clone(), false, true);
    assert_eq!(app.filmstrip_selection(), order);
    // Unknown paths (e.g. a non-RAW grid entry) leave everything alone.
    app.select_filmstrip_path(
        dir.path().join("notes.png").display().to_string(),
        false,
        false,
    );
    assert_eq!(app.filmstrip_selection(), order);
}

/// P0-Audit (DoD §3, GUI-RIGHT-THUMB-1): das Develop-Panel malt kein
/// dupliziertes Vorschaubild — maximal eine Bild-Textur.
#[test]
fn right_panel_paints_no_duplicate_thumbnail() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.set_module(Module::Develop);
    let shapes = headless_shapes(&mut app, |app, ctx| {
        egui::Panel::right("controls")
            .resizable(true)
            .default_size(320.0)
            .show(ctx, |ui| app.draw_develop_panel(ui));
    });
    let blank = egui::TextureId::default();
    let images = shapes
        .iter()
        .filter(|clipped| match &clipped.shape {
            // egui malt Bilder als texturierte Meshes (kein eigenes
            // Shape-Tag): alles mit echter Textur zählt.
            egui::Shape::Mesh(mesh) => mesh.texture_id != blank,
            _ => false,
        })
        .count();
    assert!(
        images <= 1,
        "Develop panel must paint at most one image texture, got {images}"
    );
}

/// GUI-FILMSTRIP-DUP-1: a Library grid single-click routes through the
/// shared filmstrip selection (select, no open); the grid double-click
/// opens through the shared filmstrip click path. A grid click on a
/// non-RAW entry leaves the selection alone.
#[test]
fn grid_click_routes_through_shared_selection() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    // Same-directory clicks never rescan (the rescan in `open_file` only
    // runs on directory change) — pin the workdir so the fabricated
    // entries below survive the double-click's open path.
    app.directory = dir.path().display().to_string();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
        raw_entry(dir.path(), "c.cr3"),
    ];
    let order = app.filmstrip_order();
    // Grid single-click: shared select, no open.
    app.select_filmstrip_path(order[0].clone(), false, false);
    assert_eq!(app.filmstrip_selection(), vec![order[0].clone()]);
    // Identical bookkeeping to a filmstrip plain click.
    let (expected, _) =
        LuminaApp::apply_filmstrip_click(&order, &BTreeSet::new(), None, &order[0], false, false);
    assert_eq!(
        app.filmstrip_selection()
            .into_iter()
            .collect::<BTreeSet<_>>(),
        expected,
        "grid and filmstrip clicks must share one bookkeeping"
    );
    // Grid double-click: opens through the shared click path — the
    // selection itself is synchronous (never waits for the decode).
    app.handle_filmstrip_click(order[1].clone(), false, false);
    assert!(
        app.filmstrip_selection().contains(&order[1]),
        "double-click must select through the shared path"
    );
    // A non-RAW grid entry is no selection target: nothing changes.
    let before = app.filmstrip_selection();
    app.select_filmstrip_path(
        dir.path().join("notes.png").display().to_string(),
        false,
        false,
    );
    assert_eq!(app.filmstrip_selection(), before);
}

/// GUI-FILMSTRIP-DUP-1: a duplicated source path appears exactly once per
/// view — filmstrip, navigator rail and Library grid share one deduped
/// index source.
#[test]
fn duplicate_paths_appear_once_per_view() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
        raw_entry(dir.path(), "a.cr3"),
    ];
    let indices = app.raw_entry_indices();
    assert_eq!(
        indices.len(),
        2,
        "a duplicated path must collapse to one entry, got {indices:?}"
    );
    let order = app.filmstrip_order();
    assert_eq!(order.len(), 2);
    let mut sorted = order.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        sorted.len(),
        order.len(),
        "filmstrip order must hold no duplicates"
    );
    let rail: Vec<String> = indices
        .iter()
        .map(|&i| app.entries[i].path.display().to_string())
        .collect();
    assert_eq!(rail, order, "rail and filmstrip share one index source");
}

/// R5-SELECT-1: one headless Library-view frame in a persistent context, with
/// an explicit modifier state (the production click reads `InputState.modifiers`).
fn selection_view_pass(
    ctx: &egui::Context,
    app: &mut LuminaApp,
    time: &mut f64,
    events: Vec<egui::Event>,
) {
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

/// Click the grid/survey cell registered under `id` with `modifiers` held.
fn click_cell(
    ctx: &egui::Context,
    app: &mut LuminaApp,
    time: &mut f64,
    id: egui::Id,
    modifiers: egui::Modifiers,
) {
    let pos = ctx
        .read_response(id)
        .unwrap_or_else(|| panic!("cell {id:?} must be registered"))
        .rect
        .center();
    let button = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers,
    };
    // `InputState.modifiers` is fed by `ModifiersChanged`, not by the
    // `PointerButton` payload (egui 0.36), so a held Cmd/Ctrl/Shift must be
    // announced explicitly on each frame the click is processed.
    let held = egui::Event::ModifiersChanged(modifiers);
    selection_view_pass(
        ctx,
        app,
        time,
        vec![egui::Event::PointerMoved(pos), held.clone()],
    );
    selection_view_pass(
        ctx,
        app,
        time,
        vec![egui::Event::PointerMoved(pos), held.clone(), button(true)],
    );
    selection_view_pass(ctx, app, time, vec![held, button(false)]);
    selection_view_pass(ctx, app, time, vec![]);
}

/// R5-SELECT-1: modifier clicks in the Library **grid** and **Survey** view
/// must read Cmd/Ctrl-toggle + Shift-range exactly like the filmstrip
/// (`filmstrip_frame.rs`). Both used to pass a hard-coded `false, false`, so
/// multi-selection was impossible there although the filmstrip accepted it.
#[test]
fn grid_and_survey_clicks_read_modifier_keys() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.directory = dir.path().display().to_string();
    app.entries = vec![
        raw_entry(dir.path(), "a.cr3"),
        raw_entry(dir.path(), "b.cr3"),
        raw_entry(dir.path(), "c.cr3"),
    ];
    let order = app.filmstrip_order();
    assert_eq!(order.len(), 3);
    let id_of = |app: &LuminaApp, name: &str| -> egui::Id {
        let key = app
            .entries
            .iter()
            .find(|entry| entry.name == name)
            .expect("entry registered")
            .thumb_key
            .clone();
        crate::library_sort::library_cell_id(&key)
    };
    let ctrl = egui::Modifiers {
        ctrl: true,
        ..Default::default()
    };
    for view in [LibraryView::Grid, LibraryView::Survey] {
        app.set_library_view(view);
        // Each sub-case gets a fresh context so the Survey layout (which shows
        // only the selected cells once ≥2 are selected) is the full listing
        // when the click position is resolved.
        let run = |app: &mut LuminaApp,
                   selection: BTreeSet<String>,
                   anchor: Option<String>,
                   id: egui::Id,
                   modifiers: egui::Modifiers|
         -> Vec<String> {
            app.filmstrip_selection = selection;
            app.filmstrip_anchor = anchor;
            let ctx = egui::Context::default();
            let mut time = 0.0_f64;
            selection_view_pass(&ctx, app, &mut time, vec![]);
            click_cell(&ctx, app, &mut time, id, modifiers);
            app.filmstrip_selection()
        };
        let a_id = id_of(&app, "a.cr3");
        let c_id = id_of(&app, "c.cr3");
        let a = order[0].clone();
        let c = order[2].clone();

        // Plain click selects exactly one image.
        assert_eq!(
            run(
                &mut app,
                BTreeSet::new(),
                None,
                a_id,
                egui::Modifiers::default()
            ),
            vec![a.clone()],
            "{view:?}: plain click selects exactly the clicked image"
        );

        // Cmd/Ctrl-click toggles a second one in.
        let toggled = run(
            &mut app,
            BTreeSet::from([a.clone()]),
            Some(a.clone()),
            c_id,
            ctrl,
        );
        assert_eq!(toggled.len(), 2, "{view:?}: Cmd/Ctrl-click toggles in");
        assert!(
            toggled.contains(&a) && toggled.contains(&c),
            "{view:?}: toggled set {toggled:?}"
        );

        // Shift-click fills the inclusive anchor→clicked range.
        assert_eq!(
            run(
                &mut app,
                BTreeSet::from([a.clone()]),
                Some(a.clone()),
                c_id,
                egui::Modifiers::SHIFT,
            ),
            order,
            "{view:?}: Shift-click selects the anchor→clicked range"
        );
    }
}
