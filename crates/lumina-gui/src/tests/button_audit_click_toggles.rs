//! F-100 per-button click/toggle audits tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::*;
// UX-LOOK-TOOLBAR-18: the preview tool strip and Library view tabs are icons.
use crate::icon_toolbar::ToolbarIcon;

#[test]
fn f100_panel_and_view_toggle_labels_are_exhaustive_and_distinct() {
    let mut labels: Vec<Str> = [
        ViewToggle::BlackWhite,
        ViewToggle::Clipping,
        ViewToggle::LightsOut,
    ]
    .into_iter()
    .map(view_toggle_button_label)
    .collect();
    labels.extend(
        [PanelToggle::CropMode, PanelToggle::PanelsHidden]
            .into_iter()
            .map(panel_toggle_button_label),
    );
    for label in &labels {
        assert!(!label.t().is_empty(), "button label must not be empty");
    }
    let mut texts: Vec<&str> = labels.iter().map(|label| label.t()).collect();
    let count = texts.len();
    texts.sort_unstable();
    texts.dedup();
    assert_eq!(
        texts.len(),
        count,
        "button labels must be distinct: {labels:?}"
    );
    assert_eq!(
        view_toggle_button_label(ViewToggle::BlackWhite),
        Str::TreatmentBlackWhite
    );
    assert_eq!(
        panel_toggle_button_label(PanelToggle::CropMode),
        Str::ViewToolbarCrop
    );
    assert_eq!(
        panel_toggle_button_label(PanelToggle::PanelsHidden),
        Str::ViewToolbarPanels
    );
}

/// Every display toggle that was keyboard-only must be painted as a button
/// in the preview view toolbar (`PanelToggle` + `ViewToggle` except the
/// already-buttoned B&W treatment, whose button lives in the Basic section).
#[test]
fn f100_view_toolbar_paints_every_display_toggle_button() {
    let mut app = toolbar_app();
    let (shapes, ctx) = preview_area_frame(&mut app);
    // Display-only view toggles *and* the interactive develop tools are icons
    // in the LR tool strip (UX-LOOK-TOOLBAR-18).
    for icon in [
        ToolbarIcon::Crop,
        ToolbarIcon::Heal,
        ToolbarIcon::RedEye,
        ToolbarIcon::Masking,
        ToolbarIcon::Clipping,
        ToolbarIcon::LightsOut,
        ToolbarIcon::Panels,
        ToolbarIcon::AllPanels,
        ToolbarIcon::Fullscreen,
        ToolbarIcon::Split,
    ] {
        assert_icon_painted(&shapes, &ctx, icon);
    }
    // `V` B&W: button lives in the Basic section, not the preview toolbar.
    app.set_section_open(SECTION_BASIC, true);
    let basic = headless_shapes(&mut app, |app, ui| app.draw_basic(ui));
    assert_fully_visible(&basic, view_toggle_button_label(ViewToggle::BlackWhite).t());
}

/// The known gap (F-103-N6): the crop mode was only reachable via `R`.
/// The new preview-toolbar button must toggle the mode and paint the badge.
#[test]
fn f100_crop_button_toggles_crop_mode_and_badge() {
    let mut app = toolbar_app();
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Crop], draw_preview_area_only);
    assert!(app.crop_mode, "crop button must toggle crop mode on");
    assert_eq!(app.status, Str::CropModeOn.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Crop);
    // The state badge is painted after the preview image; use the tall
    // harness so it is inside the visible canvas (below the 720px fold in
    // the normal layout, exactly like the production status row).
    let badge = preview_area_badge_shapes(&mut app);
    assert!(
        text_contains(&badge, Str::CropModeOn.t()),
        "the crop-mode badge must be visible after the button click"
    );
    // Second click turns it off again (same path as `R`).
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Crop], draw_preview_area_only);
    assert!(!app.crop_mode, "crop button must toggle crop mode off");
    assert_eq!(app.status, Str::CropModeOff.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Crop);
    let badge = preview_area_badge_shapes(&mut app);
    assert!(
        !text_contains(&badge, Str::CropModeOn.t()),
        "the crop-mode badge must disappear when toggled off"
    );
}

/// The lights-out button must be clickable even while the chrome it hides
/// is hidden (it stays visible in the central preview area) and toggles the
/// same state as `L`.
#[test]
fn f100_lights_out_button_toggles_and_stays_reachable() {
    let mut app = toolbar_app();
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::LightsOut], draw_preview_area_only);
    assert!(app.lights_out, "lights-out button must arm lights-out");
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::LightsOut);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::LightsOut], draw_preview_area_only);
    assert!(!app.lights_out, "lights-out button must disarm again");
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::LightsOut);
}

/// The remaining keyboard-only actions get buttons: `\` filter drawer,
/// `Shift+Y` split, `F` fullscreen, `Shift+Tab` all-panels, and the
/// copy/history chords.
#[test]
fn f100_keyboard_only_actions_have_buttons() {
    let mut app = toolbar_app();
    let (preview, ctx) = preview_area_frame(&mut app);
    for icon in [
        ToolbarIcon::Split,
        ToolbarIcon::Fullscreen,
        ToolbarIcon::AllPanels,
    ] {
        assert_icon_painted(&preview, &ctx, icon);
    }
    // `\` filter drawer: button in the Library grid toolbar.
    let library = headless_shapes(&mut app, |app, ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    });
    assert_fully_visible(&library, Str::FilterBar.t());
    // Copy/history chords: buttons in the (opened) History section.
    let history = headless_click_label(&mut app, Str::History.t(), |app, ui| {
        app.draw_history_section(ui)
    });
    for label in [
        Str::DuplicateCopy,
        Str::CopySettings,
        Str::PasteSettings,
        Str::SnapshotButton,
        Str::StackGroup,
    ] {
        assert_fully_visible(&history, label.t());
    }
}

#[test]
fn f100_clipping_button_toggles_overlay() {
    let mut app = toolbar_app();
    assert_preview_icon_toggles(
        &mut app,
        ToolbarIcon::Clipping,
        |app| app.clipping_overlay,
        Str::ClippingOn.t(),
        Str::ClippingOff.t(),
    );
}

#[test]
fn f100_split_button_toggles_and_holds_before() {
    let mut app = toolbar_app();
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Split], draw_preview_area_only);
    assert!(app.before_after_split, "split button must arm the marker");
    assert!(app.before_after, "split keeps the Before image held");
    assert_eq!(app.status, Str::SplitViewOn.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Split);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Split], draw_preview_area_only);
    assert!(!app.before_after_split, "split button must disarm again");
    assert_eq!(app.status, Str::SplitViewOff.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Split);
}

#[test]
fn f100_panels_button_toggles_side_panels() {
    let mut app = toolbar_app();
    assert_preview_icon_toggles(
        &mut app,
        ToolbarIcon::Panels,
        |app| app.panels_hidden,
        Str::PanelsHiddenOn.t(),
        Str::PanelsHiddenOff.t(),
    );
}

#[test]
fn f100_all_panels_button_toggles_all_panels() {
    let mut app = toolbar_app();
    assert_preview_icon_toggles(
        &mut app,
        ToolbarIcon::AllPanels,
        |app| app.all_panels_hidden(),
        Str::AllPanelsHiddenOn.t(),
        Str::AllPanelsHiddenOff.t(),
    );
}

#[test]
fn f100_fullscreen_button_toggles_and_settles_fit() {
    let mut app = toolbar_app();
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Fullscreen], draw_preview_area_only);
    assert!(app.fullscreen, "fullscreen button must arm fullscreen");
    assert_eq!(app.zoom_mode, ZoomMode::Fit);
    assert_eq!(app.status, Str::FullscreenOn.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Fullscreen);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Fullscreen], draw_preview_area_only);
    assert!(!app.fullscreen, "fullscreen button must disarm again");
    assert_eq!(app.status, Str::FullscreenOff.t());
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Fullscreen);
}

#[test]
fn f100_filter_button_toggles_drawer() {
    let mut app = toolbar_app();
    let draw_library_grid = |app: &mut LuminaApp, ui: &mut egui::Ui| {
        let ctx = ui.ctx().clone();
        app.draw_library_grid(&ctx, ui);
    };
    let shapes = headless_click_label(&mut app, Str::FilterBar.t(), draw_library_grid);
    assert!(app.filter_bar_visible, "filter button must show the drawer");
    assert_eq!(app.status, Str::FilterShown.t());
    assert_fully_visible(&shapes, Str::FilterBar.t());
    let shapes = headless_click_label(&mut app, Str::FilterBar.t(), draw_library_grid);
    assert!(
        !app.filter_bar_visible,
        "filter button must hide the drawer"
    );
    assert_eq!(app.status, Str::FilterHidden.t());
    assert_fully_visible(&shapes, Str::FilterBar.t());
}

#[test]
fn f100_history_duplicate_copy_button_duplicates() {
    let (_directory, mut app) = persistent_app();
    let before = app.document.as_ref().unwrap().virtual_copies.len();
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::DuplicateCopy.t()],
        |app, ui| app.draw_history_section(ui),
    );
    let after = app.document.as_ref().unwrap().virtual_copies.len();
    assert_eq!(
        after,
        before + 1,
        "duplicate button must add a virtual copy"
    );
    assert_fully_visible(&shapes, Str::DuplicateCopy.t());
}

#[test]
fn f100_history_copy_and_paste_buttons_roundtrip() {
    // Clipboard state lives in the session; both clicks run in their own
    // headless pass, exactly like a user closing and reopening the drawer.
    let (_directory, mut app) = persistent_app();
    let draw_history = |app: &mut LuminaApp, ui: &mut egui::Ui| app.draw_history_section(ui);
    app.set_adjustment("exposure", 2.0);
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::CopySettings.t()],
        draw_history,
    );
    assert!(
        app.clipboard_has_settings(),
        "copy button must fill the clipboard"
    );
    assert_fully_visible(&shapes, Str::CopySettings.t());
    app.set_adjustment("exposure", -1.0);
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::PasteSettings.t()],
        draw_history,
    );
    assert_eq!(app.recipe().adjustments["exposure"], 2.0);
    assert_fully_visible(&shapes, Str::PasteSettings.t());
}

#[test]
fn f100_history_snapshot_button_freezes() {
    let (_directory, mut app) = persistent_app();
    assert!(app.snapshots().is_empty());
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::SnapshotButton.t()],
        |app, ui| app.draw_history_section(ui),
    );
    assert_eq!(
        app.snapshots().len(),
        1,
        "snapshot button must freeze history"
    );
    assert_fully_visible(&shapes, Str::SnapshotButton.t());
}

#[test]
fn f100_history_stack_button_toggles_group() {
    let (_directory, mut app) = persistent_app();
    assert_eq!(app.stack_group_id(), None);
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::StackGroup.t()],
        |app, ui| app.draw_history_section(ui),
    );
    assert!(
        app.stack_group_id().is_some(),
        "stack button must group the copy"
    );
    // The same button relabels to "Unstack" after the grouping click.
    assert_fully_visible(&shapes, Str::StackUngroup.t());
    // A second click on the relabelled button ungroups again. Both clicks
    // share one context so the drawer stays open.
    let shapes = headless_click_labels(
        &mut app,
        &[Str::History.t(), Str::StackUngroup.t()],
        |app, ui| app.draw_history_section(ui),
    );
    assert_eq!(app.stack_group_id(), None, "unstack button must ungroup");
    assert_fully_visible(&shapes, Str::StackGroup.t());
}
