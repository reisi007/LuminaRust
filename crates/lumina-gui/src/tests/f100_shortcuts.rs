//! F-100 enum shortcut → button maps tests (GUI-REFACTOR-W3-20 split from the root `mod tests`).

use super::f100_audit::{f100_surface_shapes, F100Surface};
use super::*;

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

fn f100_module_button_label(module: Module) -> (F100Surface, String) {
    match module {
        Module::Library => (F100Surface::ModuleBar, Str::LibraryShortcut.format_arg("G")),
        Module::Develop => (F100Surface::ModuleBar, Str::DevelopShortcut.format_arg("D")),
        Module::Export => (F100Surface::ModuleBar, Str::Export.t().into()),
    }
}

fn f100_library_view_button_label(view: LibraryView) -> (F100Surface, String) {
    let label = match view {
        LibraryView::Grid => Str::LibraryGridOn.t(),
        LibraryView::Loupe => Str::LoupeOn.t(),
        LibraryView::Compare => Str::CompareModeCompare.t(),
        LibraryView::Survey => Str::SurveyOn.t(),
        LibraryView::People => Str::FacePeople.t(),
    };
    (F100Surface::LibraryGrid, label.into())
}

fn f100_compare_mode_button_label(mode: CompareMode) -> (F100Surface, String) {
    // The clickable alias lives in the Library view selector (`C` compares,
    // `N` surveys the grid), never as a second, diverging button.
    let label = match mode {
        CompareMode::Compare => Str::CompareModeCompare.t(),
        CompareMode::Survey => Str::SurveyOn.t(),
    };
    (F100Surface::LibraryGrid, label.into())
}

fn f100_mask_tool_button_label(tool: MaskTool) -> (F100Surface, String) {
    let label = match tool {
        MaskTool::None => Str::MaskToolNone.t(),
        MaskTool::Brush => Str::MaskToolBrush.t(),
        MaskTool::LinearGradient => Str::MaskToolGradient.t(),
        MaskTool::Radial => Str::MaskToolRadial.t(),
    };
    (F100Surface::Masking, label.into())
}

fn f100_flag_button_label(flag: Flag) -> (F100Surface, String) {
    (F100Surface::Rating, flag_label(flag).into())
}

#[test]
fn f100_shortcut_enum_variants_have_buttons() {
    let (_directory, mut app) = persistent_app();
    let mut entries: Vec<(F100Surface, String)> = Vec::new();
    for toggle in [
        ViewToggle::BlackWhite,
        ViewToggle::Clipping,
        ViewToggle::LightsOut,
    ] {
        entries.push((
            f100_view_toggle_surface(toggle),
            view_toggle_button_label(toggle).t().into(),
        ));
    }
    for toggle in [PanelToggle::CropMode, PanelToggle::PanelsHidden] {
        entries.push((
            f100_panel_toggle_surface(toggle),
            panel_toggle_button_label(toggle).t().into(),
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
        entries.push(f100_library_view_button_label(view));
    }
    for mode in [CompareMode::Compare, CompareMode::Survey] {
        entries.push(f100_compare_mode_button_label(mode));
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
        let shapes = f100_surface_shapes(&mut app, surface);
        for (entry_surface, label) in &entries {
            if *entry_surface == surface {
                assert_fully_visible(&shapes, label);
            }
        }
    }
}
