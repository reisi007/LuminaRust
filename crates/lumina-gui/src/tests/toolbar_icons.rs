//! UX-LOOK-TOOLBAR-18 (UXG-04): headless paint + click coverage for the icon
//! tool strip and the iconified Library view tabs.
//!
//! Every toolbar button must paint an icon (no text label) *and* flip its
//! state on a real headless click — the DoD §3/§5 requirement. Icons are
//! located by their stable widget id via `Context::read_response`.

use super::*;
use crate::icon_toolbar::{library_view_icon, ToolbarIcon, ALL_TOOLBAR_ICONS};

fn draw_library_grid(app: &mut LuminaApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    app.draw_library_grid(&ctx, ui);
}

#[test]
fn toolbar_icon_ids_and_tooltips_are_distinct_and_non_empty() {
    let mut ids: Vec<egui::Id> = Vec::new();
    let mut tooltips: Vec<String> = Vec::new();
    for icon in ALL_TOOLBAR_ICONS {
        assert!(!icon.key().is_empty(), "{icon:?} key must not be empty");
        assert!(
            !icon.tooltip().is_empty(),
            "{icon:?} tooltip must not be empty"
        );
        ids.push(icon.id());
        tooltips.push(icon.tooltip());
    }
    let count = ids.len();
    ids.sort_by_key(|id| id.value());
    ids.dedup();
    assert_eq!(ids.len(), count, "icon widget ids must be unique");
    let count = tooltips.len();
    tooltips.sort();
    tooltips.dedup();
    assert_eq!(tooltips.len(), count, "icon tooltips must be distinct");
}

#[test]
fn library_view_icon_mapping_is_exhaustive() {
    assert_eq!(library_view_icon(LibraryView::Grid), ToolbarIcon::ViewGrid);
    assert_eq!(
        library_view_icon(LibraryView::Loupe),
        ToolbarIcon::ViewLoupe
    );
    assert_eq!(
        library_view_icon(LibraryView::Compare),
        ToolbarIcon::ViewCompare
    );
    assert_eq!(
        library_view_icon(LibraryView::Survey),
        ToolbarIcon::ViewSurvey
    );
    assert_eq!(
        library_view_icon(LibraryView::People),
        ToolbarIcon::ViewPeople
    );
}

#[test]
fn toolbar_paints_all_interactive_tools() {
    let mut app = toolbar_app();
    let (shapes, ctx) = preview_area_frame(&mut app);
    for icon in [
        ToolbarIcon::Crop,
        ToolbarIcon::Heal,
        ToolbarIcon::RedEye,
        ToolbarIcon::Masking,
    ] {
        assert_icon_painted(&shapes, &ctx, icon);
    }
}

#[test]
fn toolbar_heal_button_toggles_spot_tool() {
    let mut app = toolbar_app();
    assert_eq!(app.spot_tool(), SpotTool::None);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Heal], draw_preview_area_only);
    assert_eq!(
        app.spot_tool(),
        SpotTool::Heal,
        "Heal click must arm the tool"
    );
    assert_eq!(app.status, "Spot heal armed (Q)");
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Heal);
    let (_shapes, _ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Heal], draw_preview_area_only);
    assert_eq!(app.spot_tool(), SpotTool::None, "second Heal click disarms");
    assert_eq!(app.status, "Spot heal disarmed");
}

#[test]
fn toolbar_red_eye_button_toggles_pick_mode() {
    let mut app = toolbar_app();
    assert!(!app.red_eye_pick_mode);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::RedEye], draw_preview_area_only);
    assert!(app.red_eye_pick_mode, "Red-Eye click must arm the picker");
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::RedEye);
    let (_shapes, _ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::RedEye], draw_preview_area_only);
    assert!(!app.red_eye_pick_mode, "second Red-Eye click disarms");
}

#[test]
fn toolbar_masking_button_toggles_brush() {
    let mut app = toolbar_app();
    assert_eq!(app.mask_tool, MaskTool::None);
    let (shapes, ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Masking], draw_preview_area_only);
    assert_eq!(
        app.mask_tool,
        MaskTool::Brush,
        "Masking click arms the brush"
    );
    assert_icon_painted(&shapes, &ctx, ToolbarIcon::Masking);
    let (_shapes, _ctx) =
        headless_click_icons_frame(&mut app, &[ToolbarIcon::Masking], draw_preview_area_only);
    assert_eq!(
        app.mask_tool,
        MaskTool::None,
        "second Masking click disarms"
    );
}

#[test]
fn library_view_tab_icons_paint_and_switch_the_view() {
    let (_directory, mut app) = persistent_app();
    // All five tabs paint (empty or not, the selector row is drawn first).
    let (shapes, ctx) = headless_frame(&mut app, draw_library_grid);
    for icon in [
        ToolbarIcon::ViewGrid,
        ToolbarIcon::ViewLoupe,
        ToolbarIcon::ViewCompare,
        ToolbarIcon::ViewSurvey,
        ToolbarIcon::ViewPeople,
    ] {
        assert_icon_painted(&shapes, &ctx, icon);
    }
    // Each tab click routes through `set_library_view`.
    for (icon, view) in [
        (ToolbarIcon::ViewLoupe, LibraryView::Loupe),
        (ToolbarIcon::ViewCompare, LibraryView::Compare),
        (ToolbarIcon::ViewSurvey, LibraryView::Survey),
        (ToolbarIcon::ViewPeople, LibraryView::People),
        (ToolbarIcon::ViewGrid, LibraryView::Grid),
    ] {
        let (_shapes, _ctx) = headless_click_icons_frame(&mut app, &[icon], draw_library_grid);
        assert_eq!(app.library_view(), view, "{icon:?} must switch the view");
    }
}
