//! F-100 enum shortcut → button maps tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::f100_audit::{f100_assert_button, f100_surface_frame};
use super::f100_surface::{ButtonRef, F100Surface};
use super::*;
// UX-LOOK-TOOLBAR-18: the preview tool strip and the Library view tabs are
// icon-based; their buttons are located by widget id.
use crate::icon_toolbar::{library_view_icon, ToolbarIcon};

// Exhaustive per-enum shortcut → button maps (no `_` arm). Together with
// the `GuiAction` audit above these cover every F-100 shortcut: `G`/`D`/`E`
// (Module/LibraryView), `C`/`N` (CompareMode), `K`/`M`/`Shift+M`
// (MaskTool) and `P`/`X`/`U` (Flag). Numeric rating/color-label shortcuts
// share their `GuiAction` buttons (`1`–`5`/`0` → SetRating, `6`–`9` →
// SetColorLabel), covered by the audit above.

fn f100_view_toggle_surface(toggle: ViewToggle) -> F100Surface {
    match toggle {
        ViewToggle::BlackWhite => F100Surface::Basic,
        ViewToggle::Clipping | ViewToggle::LightsOut => F100Surface::Preview,
    }
}

fn f100_panel_toggle_surface(toggle: PanelToggle) -> F100Surface {
    match toggle {
        PanelToggle::CropMode | PanelToggle::PanelsHidden => F100Surface::Preview,
    }
}

/// UX-LOOK-TOOLBAR-18: the clickable button for one view toggle (text in the
/// Basic section for B&W; icons in the preview tool strip for the rest).
fn f100_view_toggle_button(toggle: ViewToggle) -> ButtonRef {
    match toggle {
        ViewToggle::BlackWhite => ButtonRef::Text(view_toggle_button_label(toggle).t().into()),
        ViewToggle::Clipping => ButtonRef::Icon(ToolbarIcon::Clipping),
        ViewToggle::LightsOut => ButtonRef::Icon(ToolbarIcon::LightsOut),
    }
}

fn f100_panel_toggle_button(toggle: PanelToggle) -> ButtonRef {
    match toggle {
        PanelToggle::CropMode => ButtonRef::Icon(ToolbarIcon::Crop),
        PanelToggle::PanelsHidden => ButtonRef::Icon(ToolbarIcon::Panels),
    }
}

fn f100_module_button_label(module: Module) -> (F100Surface, ButtonRef) {
    match module {
        Module::Library => (
            F100Surface::ModuleBar,
            Str::LibraryShortcut.format_arg("G").into(),
        ),
        Module::Develop => (
            F100Surface::ModuleBar,
            Str::DevelopShortcut.format_arg("D").into(),
        ),
        Module::Export => (F100Surface::ModuleBar, Str::Export.t().into()),
    }
}

fn f100_library_view_button(view: LibraryView) -> (F100Surface, ButtonRef) {
    (
        F100Surface::LibraryGrid,
        ButtonRef::Icon(library_view_icon(view)),
    )
}

fn f100_compare_mode_button(mode: CompareMode) -> (F100Surface, ButtonRef) {
    // The clickable alias lives in the Library view selector (`C` compares,
    // `N` surveys the grid), never as a second, diverging button.
    let view = match mode {
        CompareMode::Compare => LibraryView::Compare,
        CompareMode::Survey => LibraryView::Survey,
    };
    (
        F100Surface::LibraryGrid,
        ButtonRef::Icon(library_view_icon(view)),
    )
}

fn f100_mask_tool_button_label(tool: MaskTool) -> (F100Surface, ButtonRef) {
    let label = match tool {
        MaskTool::None => Str::MaskToolNone.t(),
        MaskTool::Brush => Str::MaskToolBrush.t(),
        MaskTool::LinearGradient => Str::MaskToolGradient.t(),
        MaskTool::Radial => Str::MaskToolRadial.t(),
    };
    (F100Surface::Masking, label.into())
}

fn f100_flag_button_label(flag: Flag) -> (F100Surface, ButtonRef) {
    (F100Surface::Rating, flag_label(flag).into())
}

#[test]
fn f100_shortcut_enum_variants_have_buttons() {
    let (_directory, mut app) = persistent_app();
    let mut entries: Vec<(F100Surface, ButtonRef)> = Vec::new();
    for toggle in [
        ViewToggle::BlackWhite,
        ViewToggle::Clipping,
        ViewToggle::LightsOut,
    ] {
        entries.push((
            f100_view_toggle_surface(toggle),
            f100_view_toggle_button(toggle),
        ));
    }
    for toggle in [PanelToggle::CropMode, PanelToggle::PanelsHidden] {
        entries.push((
            f100_panel_toggle_surface(toggle),
            f100_panel_toggle_button(toggle),
        ));
    }
    for module in [Module::Library, Module::Develop, Module::Export] {
        entries.push(f100_module_button_label(module));
    }
    for view in [
        LibraryView::Grid,
        LibraryView::Loupe,
        LibraryView::Compare,
        LibraryView::Survey,
        LibraryView::People,
    ] {
        entries.push(f100_library_view_button(view));
    }
    for mode in [CompareMode::Compare, CompareMode::Survey] {
        entries.push(f100_compare_mode_button(mode));
    }
    for tool in [
        MaskTool::None,
        MaskTool::Brush,
        MaskTool::LinearGradient,
        MaskTool::Radial,
    ] {
        entries.push(f100_mask_tool_button_label(tool));
    }
    for flag in [Flag::Pick, Flag::Reject, Flag::Unflagged] {
        entries.push(f100_flag_button_label(flag));
    }
    for surface in [
        F100Surface::Preview,
        F100Surface::Rating,
        F100Surface::LibraryGrid,
        F100Surface::ModuleBar,
        F100Surface::Basic,
        F100Surface::Masking,
    ] {
        let (shapes, ctx) = f100_surface_frame(&mut app, surface);
        for (entry_surface, button) in &entries {
            if *entry_surface == surface {
                f100_assert_button(&shapes, &ctx, button, "shortcut-map entry");
            }
        }
    }
}
