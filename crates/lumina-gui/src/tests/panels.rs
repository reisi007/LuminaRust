//! panel layout and button-inside-panel audits tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;

/// GUI-VISION-1: the Export "Choose…" button must not overflow the right
/// panel edge (kittest `export_module` golden).
#[test]
fn export_choose_button_fully_inside_panel() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.set_module(Module::Export);
    let shapes = headless_shapes(&mut app, |app, ctx| {
        egui::Panel::right("controls")
            .resizable(true)
            .default_size(320.0)
            .show(ctx, |ui| app.draw_export_panel(ui));
    });
    assert_fully_visible(&shapes, Str::ExportChoose.t());
}

/// GUI-VISION-1: the Develop "Save Recipe / Sidecar" button (now in a
/// pinned footer below the scroll area) and the path "Load" button must
/// not be cut at the panel edges (kittest `develop_basic`,
/// `histogram_graphic` goldens).
#[test]
fn develop_save_button_fully_inside_panel() {
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
    assert_fully_visible(&shapes, Str::SaveRecipe.t());
    assert_fully_visible(&shapes, Str::Load.t());
}

/// GUI-VISION-1 (same bug class): the Library folder-tree "Open" button
/// shares its row with a path field and must stay inside the panel.
#[test]
fn library_open_button_fully_inside_panel() {
    let mut app = new_app();
    let shapes = headless_shapes(&mut app, |app, ctx| {
        egui::Panel::left("folders")
            .resizable(true)
            .default_size(220.0)
            .show(ctx, |ui| app.draw_folder_tree(ui));
    });
    assert_fully_visible(&shapes, Str::Open.t());
}

/// GUI-VISION-1 (same bug class): the Masking "New Mask" button shares
/// its row with a name field and must stay inside the panel. The row only
/// renders with a loaded document, and the Masking section starts
/// collapsed — so the test prepares an in-memory document (no disk
/// writes), draws the Masking section directly in a right panel with the
/// production parameters, and opens it with a synthetic header click.
///
/// Two assertions: `assert_fully_visible` (1:1 pattern of the other
/// clip tests — the button must not be cut) plus a panel-width gate.
/// The width gate is the discriminating one here: an unbounded field
/// makes the row demand more than the 320px default, which widens the
/// panel in this harness (measured 390px pre-fix) and — with the panel
/// held at 320px by the full app layout — clips the button in
/// production (the export_module `Choose…` finding).
#[test]
fn masking_new_button_fully_inside_panel() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.set_module(Module::Develop);
    // In-memory document only (`SidecarDocument::new`, no save): enough
    // for `draw_masking` to render past its `document.clone()` guard.
    // The mask library stays empty so no long mask name can widen the
    // panel on its own behalf.
    app.ensure_document_loaded().unwrap();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 720.0));
    // Simulated clock: the open/close animation only progresses while
    // time advances, so every frame steps it by 1/60s.
    let mut t = 0.0;
    let mut panel_rect = egui::Rect::NOTHING;
    let mut run = |events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(t),
                events,
                ..Default::default()
            },
            |ui| {
                let r = egui::Panel::right("controls")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| app.draw_masking(ui));
                panel_rect = r.response.rect;
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    // Frame 1: layout; locate the Masking header.
    let shapes = run(vec![]);
    let pos = text_shapes_for(&shapes, Str::Masking.t())
        .into_iter()
        .next()
        .expect("Masking header must be painted")
        .0
        .center();
    // Press + release on the header to open the section (`clicked()`
    // fires on release), then settle the open animation (~0.3s) and the
    // panel width so the final frame is representative.
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(vec![egui::Event::PointerMoved(pos), click(true)]);
    run(vec![egui::Event::PointerMoved(pos), click(false)]);
    let mut shapes = Vec::new();
    for _ in 0..30 {
        shapes = run(vec![]);
    }
    assert_fully_visible(&shapes, Str::NewMask.t());
    assert!(
        panel_rect.width() <= 321.0,
        "New Mask row must not push the panel past its 320px default (got {panel_rect:?})"
    );
}

/// GUI-LENSFUN-GATE-4 (optional, same bug class as
/// `masking_new_button_fully_inside_panel`): the Metadata draft editor's
/// four actions (`Save/Clear/Copy/Paste`) must not widen the resizable
/// right panel past its 320 px default. KITTEST-COVERAGE-STATES-2 measured
/// ~353 px for the unwrapped row, which reflowed the centre at 1024 px;
/// the `horizontal_wrapped` fix must keep the panel and every action label
/// inside it. Tall synthetic screen so the whole draft editor (all fields
/// plus the action row) fits the panel height without scrolling.
#[test]
fn metadata_panel_stays_inside_default_width() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.ensure_document_loaded().unwrap();
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1024.0, 1200.0));
    // Simulated clock: the open/close animation only progresses while
    // time advances, so every frame steps it by 1/60s.
    let mut t = 0.0;
    let mut panel_rect = egui::Rect::NOTHING;
    let mut run = |events: Vec<egui::Event>| {
        t += 1.0 / 60.0;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                time: Some(t),
                events,
                ..Default::default()
            },
            |ui| {
                let r = egui::Panel::right("controls")
                    .resizable(true)
                    .default_size(320.0)
                    .show(ui, |ui| app.draw_library_metadata_panel(ui));
                panel_rect = r.response.rect;
            },
        );
        output.textures_delta.clear();
        output.shapes
    };
    // Frame 1: layout; locate the "Metadata draft" collapsing header.
    let shapes = run(vec![]);
    let pos = text_shapes_for(&shapes, Str::MetadataDraftSection.t())
        .into_iter()
        .next()
        .expect("Metadata draft header must be painted")
        .0
        .center();
    let click = |pressed: bool| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    run(vec![egui::Event::PointerMoved(pos), click(true)]);
    run(vec![egui::Event::PointerMoved(pos), click(false)]);
    let mut shapes = Vec::new();
    for _ in 0..30 {
        shapes = run(vec![]);
    }
    for label in [
        Str::MetadataSaveDraft.t(),
        Str::MetadataClearDraft.t(),
        Str::MetadataCopyDraft.t(),
        Str::MetadataPasteDraft.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }
    assert!(
        panel_rect.width() <= 321.0,
        "Metadata draft actions must not push the panel past its 320px default (got {panel_rect:?})"
    );
}

/// LRPAR-G14-DENOISE-IMPL-20 (GUI): the Detail section paints the denoise
/// controls, the readable model identity and the status badge for every
/// state; the pending model is the honest `unavailable` badge.
#[test]
fn denoise_panel_paints_controls_identity_and_status() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_section_open(SECTION_DETAIL, true);
    app.set_denoise_enabled(true).unwrap();
    // A tall virtual canvas so the whole Detail section is painted (the
    // production panel scrolls; there is no scroll driver in a one-shot
    // headless frame).
    let shapes = headless_shapes_sized(&mut app, 4096.0, |app, ui| app.draw_develop_panel(ui));
    for label in [
        Str::DenoiseAi.t(),
        Str::DenoiseEnable.t(),
        Str::DenoiseStrength.t(),
        Str::DenoisePreserveDetail.t(),
        Str::DenoiseStatusUnavailable.t(),
        Str::DenoisePolicyWarn.t(),
        Str::DenoisePolicyStrict.t(),
        Str::DenoiseNotReadyWarning.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }
    // The readable model identity line is painted with the pending hash.
    assert!(
        shapes.iter().any(|clipped| matches!(
            &clipped.shape,
            egui::Shape::Text(text)
                if text.galley.text().contains("pending-integration")
        )),
        "the model identity must name the persisted hash"
    );
}

/// LRPAR-G09-CULL-25 + LRPAR-G13-MERGE-15: the Library metadata panel
/// carries the assisted-culling and merge sub-sections (collapsed headers
/// keep the 320px default width).
#[test]
fn library_panel_paints_culling_and_merge_sections() {
    let mut app = new_app();
    let shapes = headless_shapes(&mut app, |app, ui| app.draw_library_metadata_panel(ui));
    assert_fully_visible(&shapes, Str::CullingSection.t());
    assert_fully_visible(&shapes, Str::MergeSection.t());
}

/// LRPAR-G12-FACE-20 (S5): the Library view selector offers the People
/// view and the view paints its status + filter for a loaded image.
#[test]
fn library_people_view_paints_from_the_selector() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_library_view(LibraryView::People);
    assert_eq!(app.library_view(), LibraryView::People);
    let shapes = headless_shapes(&mut app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui)
    });
    // Without a face analysis the view paints the status + honest hint
    // (the filter row only appears once an analysis exists).
    let status_line = Str::FaceStatusPattern.format_arg(Str::FaceNoAnalysis.t());
    for label in [
        Str::FacePeople.t(),
        status_line.as_str(),
        Str::FaceNoAnalysisHint.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }
}

/// GUI-VISION-1 refactor guard: the Develop panel pins its footer in a bottom
/// `Panel` (LAYOUT-V1; previously a bottom-up shell) but the scroll content
/// must stay top-down in F-100 order — headers paint top-to-bottom
/// Basic → … → Masking.
#[test]
fn develop_sections_stay_top_down_with_pinned_footer() {
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
    let top_y = |needle: &str| {
        text_shapes_for(&shapes, needle)
            .iter()
            .map(|(rect, _)| rect.min.y)
            .min_by(f32::total_cmp)
            .unwrap_or_else(|| panic!("{needle:?} must be painted"))
    };
    let mut last = f32::NEG_INFINITY;
    for section in [
        "Basic",
        "Tone Curve",
        "Color",
        "Detail",
        "Effects",
        "Optics",
        "Geometry",
        "Masking",
    ] {
        let y = top_y(section);
        assert!(
            y > last,
            "{section:?} (y={y}) must paint below the previous section (y={last})"
        );
        last = y;
    }
    // The footer is pinned at the very bottom: Save paints below Masking.
    assert!(
        top_y(Str::SaveRecipe.t()) > last,
        "Save footer must paint below the last section"
    );
}
