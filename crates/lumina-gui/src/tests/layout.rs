//! UX-LOOK-LAYOUT-18 (Release 1.0): Develop left-rail macro layout and the
//! right-anchored per-section Previous/Reset row. Headless assertions (egui
//! `Context` + `LuminaApp`, tempdir fixtures) for the pure layout slice: the
//! rail paints Navigator + Presets + Snapshots + History + Copy/Paste, the
//! Previous/Reset pair sits at the right panel edge and both buttons stay
//! clickable (state changes, never a silent no-op). No recipe/sidecar
//! behaviour is asserted differently than before — the actions route through
//! the existing handlers.

use super::*;

/// UX-LOOK-LAYOUT-18: the Develop left rail paints every Lightroom-Classic
/// left panel — Navigator, Presets, Snapshots and History — plus the
/// Copy/Paste admin buttons in the History action row, and lists a created
/// snapshot by name. The `Presets`/`Snapshot`/`History` headers start
/// collapsed (egui 0.36 default), so the test opens the two panels under test
/// through their real headers, exactly like a user.
#[test]
fn develop_left_rail_paints_navigator_presets_snapshots_history() {
    let (_directory, mut app) = persistent_app();
    app.set_module(Module::Develop);
    app.create_snapshot("Rail Snapshot").unwrap();

    let draw = |app: &mut LuminaApp, ui: &mut egui::Ui| {
        let ctx = ui.ctx().clone();
        app.draw_develop_left_rail(&ctx, ui);
    };
    // Snapshot first (only the Snapshots header carries the label while
    // History is closed), then History — the settled frame has both open.
    let shapes = headless_click_labels_sized(
        &mut app,
        4096.0,
        &[Str::SnapshotButton.t(), Str::History.t()],
        draw,
    );

    // The rail composition: every panel header paints (Navigator is always
    // open, the others are collapsing headers).
    for label in [
        Str::Navigator.t(),
        Str::PresetsSection.t(),
        Str::SnapshotButton.t(),
        Str::History.t(),
    ] {
        assert_fully_visible(&shapes, label);
    }
    // Copy/Paste live in the History action row (F-100 buttons).
    assert_fully_visible(&shapes, Str::CopySettings.t());
    assert_fully_visible(&shapes, Str::PasteSettings.t());
    // The created snapshot is listed by name.
    assert_fully_visible(&shapes, "Rail Snapshot");
}

/// UX-LOOK-LAYOUT-18: clicking a listed snapshot restores its frozen recipe
/// through the existing `restore_snapshot` path (the recipe edited afterwards
/// is discarded back to the snapshot value). Proves the rail entry is a real,
/// wired control, not painted-only.
#[test]
fn develop_left_rail_snapshot_entry_restores_recipe() {
    let (_directory, mut app) = persistent_app();
    app.set_adjustment("exposure", 1.25);
    app.create_snapshot("Layout Snap").unwrap();
    // Diverging edit after the freeze: restoring must undo it.
    app.set_adjustment("exposure", -0.5);

    headless_click_labels(
        &mut app,
        &[Str::SnapshotButton.t(), "Layout Snap"],
        |app, ui| app.draw_snapshots_section(ui),
    );
    assert!(app.error().is_none(), "snapshot restore must not error");
    assert_eq!(
        app.recipe().adjustments.get("exposure"),
        Some(&1.25),
        "the snapshot entry must restore the frozen recipe"
    );
}

/// UX-LOOK-LAYOUT-18: the per-panel `Previous | Reset` pair is anchored at the
/// right edge of the panel body (Lightroom Classic) while still painting
/// inside the panel — `Reset` at the far right, `Previous` to its left.
#[test]
fn develop_section_previous_reset_are_right_anchored() {
    let mut app = new_app();
    app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
        .unwrap();
    app.set_section_open(SECTION_BASIC, true);
    let ctx = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 720.0));
    let mut panel_rect = egui::Rect::NOTHING;
    let mut output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(screen),
            time: Some(1.0),
            ..Default::default()
        },
        |ui| {
            let response = egui::Panel::right("controls")
                .resizable(true)
                .default_size(320.0)
                .show(ui, |ui| app.draw_basic(ui));
            panel_rect = response.response.rect;
        },
    );
    output.textures_delta.clear();
    let shapes = output.shapes;
    let previous = text_shapes_for(&shapes, Str::Previous.t())
        .into_iter()
        .next()
        .expect("Previous must be painted")
        .0;
    let reset = text_shapes_for(&shapes, Str::Reset.t())
        .into_iter()
        .next()
        .expect("Reset must be painted")
        .0;
    assert!(
        previous.center().x > panel_rect.center().x,
        "Previous must sit in the right half of the panel (got {previous:?} in {panel_rect:?})"
    );
    assert!(
        reset.center().x > panel_rect.center().x,
        "Reset must sit in the right half of the panel (got {reset:?} in {panel_rect:?})"
    );
    assert!(
        reset.min.x >= previous.max.x - 1.0,
        "Reset must sit right of Previous (got Previous {previous:?}, Reset {reset:?})"
    );
    assert!(
        reset.max.x <= panel_rect.max.x + 1.0,
        "Reset must stay inside the panel (got {reset:?} in {panel_rect:?})"
    );
}

/// LAYOUT-V1 (Follow-up UX-LOOK-LAYOUT-18): the pinned Develop footer
/// (commit row + maintenance row + Reset-Sliders checkbox) must stay readable
/// in narrow panels. Every footer button paints fully inside its clip and no
/// two button rects overlap — the maintenance row wraps into clean extra
/// lines instead of spilling into the commit row (the vision finding
/// "…Missing der / Apply" in `develop_section_history`/`_presets`/
/// `navigator_closed`). Pinned at both the default 320px panel and a tighter
/// 220px panel; the layout must be width-driven, not style-dependent.
#[test]
fn develop_footer_buttons_never_overlap_in_narrow_panels() {
    for width in [320.0_f32, 220.0] {
        let mut app = new_app();
        app.load_bytes(LuminaApp::sample_image_png(), "sample.png")
            .unwrap();
        app.set_module(Module::Develop);
        let shapes = headless_shapes(&mut app, |app, ctx| {
            egui::Panel::right("controls")
                .resizable(false)
                .exact_size(width)
                .show(ctx, |ui| app.draw_develop_panel(ui));
        });
        // Same five footer buttons in `draw_develop_panel`'s top-down order.
        let labels = [
            Str::Reset.t(),
            Str::MatchExposure.t(),
            Str::RegenerateStale.t(),
            Str::SaveRecipe.t(),
            Str::RenderApply.t(),
        ];
        let mut rects = Vec::new();
        for label in labels {
            assert_fully_visible(&shapes, label);
            let rect = text_shapes_for(&shapes, label)
                .into_iter()
                .next()
                .unwrap_or_else(|| panic!("{label:?} must be painted"))
                .0;
            assert!(
                rect.is_positive() && rect.is_finite(),
                "{label:?} must paint a valid rect, got {rect:?}"
            );
            rects.push((label, rect));
        }
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let (left_label, left) = rects[i];
                let (right_label, right) = rects[j];
                let overlap = left.intersect(right);
                assert!(
                    overlap.width() <= 0.5 || overlap.height() <= 0.5,
                    "footer buttons {left_label:?} {left:?} and {right_label:?} \
                     {right:?} overlap at a {width}px panel ({overlap:?})"
                );
            }
        }
    }
}

/// UX-LOOK-LAYOUT-18: the right-anchored Previous/Reset buttons are real
/// buttons — a headless click changes the recipe (uncommitted edit → Previous
/// restores the load baseline; Reset sets documented defaults).
#[test]
fn develop_section_previous_and_reset_buttons_change_recipe() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("photo.png");
    save_png(&source);
    let mut app = new_app();
    open_and_decode(&mut app, source.display().to_string());
    app.set_section_open(SECTION_BASIC, true);

    // Uncommitted edit → Previous restores the load baseline (absent).
    app.set_adjustment("exposure", 1.5);
    let shapes = headless_click_label(&mut app, Str::Previous.t(), |app, ui| app.draw_basic(ui));
    assert_fully_visible(&shapes, Str::Previous.t());
    assert!(app.error().is_none());
    assert!(!app.recipe().adjustments.contains_key("exposure"));

    // Reset sets the documented defaults.
    app.set_adjustment("exposure", 2.0);
    let shapes = headless_click_label(&mut app, Str::Reset.t(), |app, ui| app.draw_basic(ui));
    assert_fully_visible(&shapes, Str::Reset.t());
    assert!(app.error().is_none());
    assert_eq!(app.recipe().adjustments.get("exposure"), Some(&0.0));
}
