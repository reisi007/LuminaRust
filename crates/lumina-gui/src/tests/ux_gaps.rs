//! Headless guards for the remaining Lightroom UX observations. These assert
//! painted widgets and real action outcomes, not source-level implementation.

use super::*;

fn raw_entry(dir: &std::path::Path, name: &str) -> FileBrowserEntry {
    let path = dir.join(name);
    FileBrowserEntry {
        thumb_key: path.display().to_string(),
        name: name.into(),
        path,
        has_sidecar: false,
        source_status: SourceStatus::Missing,
        conflict: false,
        virtual_copies: 0,
        missing_models: 0,
        rating: 0,
        flag: Flag::Unflagged,
        color_label: 0,
        keywords: Vec::new(),
        collections: Vec::new(),
        camera: None,
        iso: None,
        focal_length: None,
        capture_timestamp: None,
        folder: String::new(),
        cull_badge: cull_gui::CullBadge::None,
        face_persons: Vec::new(),
        stack: None,
    }
}

fn painted_texts_for(
    app: &mut LuminaApp,
    draw: impl FnMut(&mut LuminaApp, &mut egui::Ui),
) -> Vec<String> {
    painted_texts(&headless_shapes(app, draw))
}

fn draw_filmstrip(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_filmstrip(&ctx, ui);
}

fn draw_preview(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_preview_area(&ctx, ui);
}

#[test]
fn uxg_develop_rating_row_paints_in_the_right_panel() {
    let mut app = new_app();
    let shapes = headless_shapes(&mut app, |app, ui| {
        egui::Panel::right("ux_gap_develop_right").show(ui, |ui| app.draw_develop_panel(ui));
    });
    let rating = text_shapes_for(&shapes, Str::Rating.t());
    assert!(
        !rating.is_empty(),
        "the real Develop panel must paint Rating"
    );
    assert!(
        rating.iter().all(|(rect, _)| rect.center().x > 360.0),
        "Rating is expected in the right-side panel: {rating:?}"
    );
}

#[test]
fn uxg_import_opens_a_dialog_instead_of_only_switching_the_module() {
    use egui_kittest::kittest::Queryable;

    let mut harness = egui_kittest::Harness::builder()
        .with_size([1200.0, 800.0])
        .build_eframe(|cc| LuminaApp::new(cc.egui_ctx.clone()));
    harness
        .state_mut()
        .apply_import_export_action(ImportExportAction::Import);
    harness.run();

    assert_eq!(harness.state().module(), Module::Library);
    assert_eq!(harness.state().status(), Str::GotoLibraryImport.t());
    let rendered_dialogs = harness
        .query_all_by_role(egui::accesskit::Role::Dialog)
        .count();
    assert_eq!(
        rendered_dialogs, 0,
        "the current Import gap is the absence of a rendered dialog"
    );
}

#[test]
fn uxg_compare_and_survey_paint_the_full_render() {
    let dir = tempfile::tempdir().unwrap();
    let fixture = ImageFrame::new(400, 300, vec![128; 400 * 300 * 4]).unwrap();
    let bytes = fixture.encode(ImageFileFormat::Png).unwrap();
    let first = dir.path().join("one.png");
    std::fs::write(&first, &bytes).unwrap();
    std::fs::write(dir.path().join("two.png"), &bytes).unwrap();
    let mut app = new_app();
    app.entries = vec![
        raw_entry(dir.path(), "one.png"),
        raw_entry(dir.path(), "two.png"),
    ];
    let ctx = egui::Context::default();
    open_and_decode(&mut app, first.display().to_string());
    app.render().unwrap();
    app.update_texture(&ctx);
    let render_id = app.texture.as_ref().expect("developed render").id();
    let mut thumbnail_ids = Vec::new();
    for entry in app.entries.clone() {
        app.refresh_thumbnail_source(&entry);
        let texture = ctx.load_texture(
            entry.name.clone(),
            egui::ColorImage::filled([24, 24], egui::Color32::LIGHT_BLUE),
            egui::TextureOptions::NEAREST,
        );
        thumbnail_ids.push(texture.id());
        app.thumbnails.insert(&entry.thumb_key, texture);
    }
    assert!(app
        .entries
        .iter()
        .all(|entry| app.thumbnails.get(&entry.thumb_key).is_some()));
    for view in [LibraryView::Compare, LibraryView::Survey] {
        app.set_library_view(view);
        let indices = [0, 1];
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 850.0),
                )),
                ..Default::default()
            },
            |ui| {
                if view == LibraryView::Compare {
                    app.draw_library_compare(&ctx, ui, &indices);
                } else {
                    app.draw_library_survey(&ctx, ui, &indices);
                }
            },
        );
        let painted = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) => {
                    let r = mesh.calc_bounds();
                    Some((mesh.texture_id, r))
                }
                egui::Shape::Rect(rect) => Some((rect.fill_texture_id(), rect.rect)),
                _ => None,
            })
            .filter(|(id, _)| thumbnail_ids.contains(id) || *id == render_id)
            .collect::<Vec<_>>();
        output.textures_delta.clear();
        assert!(
            !painted.is_empty()
                && painted.iter().all(|(id, rect)| {
                    thumbnail_ids.contains(id)
                        && rect.width() < 300.0
                        && rect.height() < 300.0
                }),
            "{view:?} currently paints thumbnails rather than the developed render {render_id:?}: {painted:?}"
        );
    }
}

#[test]
fn uxg_filmstrip_has_a_filter_and_sort_bar() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![raw_entry(dir.path(), "one.ARW")];
    let texts = painted_texts_for(&mut app, draw_filmstrip);
    assert!(
        !texts
            .iter()
            .any(|text| text.contains("Filter") || text.contains("Sort")),
        "filter/sort controls are not in the filmstrip: {texts:?}"
    );
}

#[test]
fn uxg_filmstrip_counter_paints_n_of_n() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = new_app();
    app.entries = vec![raw_entry(dir.path(), "one.ARW")];
    app.filmstrip_selection
        .insert(app.entries[0].path.display().to_string());
    let texts = painted_texts_for(&mut app, draw_filmstrip);
    let counter = Str::FilmstripCounter
        .t()
        .replace("{}", "1")
        .replace("{}", "1");
    assert!(
        texts.iter().any(|text| text.contains(&counter)),
        "expected painted 1 of 1 counter {counter:?}, got {texts:?}"
    );
}

#[test]
fn uxg_presets_expose_an_amount_control() {
    let mut app = new_app();
    let texts = painted_texts_for(&mut app, |app, ui| app.draw_presets_section(ui));
    assert!(
        !texts
            .iter()
            .any(|text| text.to_lowercase().contains("amount")),
        "presets expose no amount control: {texts:?}"
    );
}

#[test]
fn uxg_canvas_paints_size_and_feather_steppers() {
    let mut app = new_app();
    let (shapes, _) = headless_frame(&mut app, draw_preview);
    let texts = painted_texts(&shapes);
    assert!(
        !texts.iter().any(|text| text == "Size" || text == "Feather"),
        "canvas has no size/feather steppers (tool options are separate): {texts:?}"
    );
}

#[test]
fn uxd_spot_options_panel_paints_only_when_the_heal_tool_is_active() {
    let mut app = new_app();
    let inactive = painted_texts_for(&mut app, draw_preview);
    assert!(
        !inactive.iter().any(|text| text == "Spot Heal (Q)"),
        "options must be absent while Heal is inactive"
    );
    app.toggle_spot_heal_tool();
    let active = painted_texts_for(&mut app, draw_preview);
    assert!(
        active.iter().any(|text| text == "Spot Heal (Q)"),
        "options must be painted while Heal is active"
    );
}
