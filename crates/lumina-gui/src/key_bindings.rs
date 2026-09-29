//! The **keyboard shortcut table** of the native GUI: *which key does what*.
//!
//! This is a file-size-ratchet extraction from `lib.rs` along one concern
//! only. Every item here answers the same question — which key, with which
//! modifier state, maps to which action — and every mapping is a pure
//! function over `egui::Key` plus the modifier flags its caller already
//! holds, which is what makes the whole table testable without an
//! [`egui::Context`]. Each result enum sits next to its mapping on purpose: an
//! enum that only ever occurs as a mapping's return value carries no meaning
//! apart from that mapping, and where such an enum is *also* the app's module
//! or view state ([`Module`], [`LibraryView`]) the key and the state are one
//! decision, documented as one.
//!
//! The table was scattered over four places in `lib.rs`; the extraction is a
//! pure relocation — every item is re-exported from the crate root, so no call
//! site in any crate changed, and not one doc comment was reworded.
//!
//! Three neighbours deliberately stay in `lib.rs`:
//!
//! * `color_label_of`, `compare_mode_for_key` and `mask_tool_for_key` document
//!   themselves through the `LuminaApp` write path they feed
//!   ([`crate::LuminaApp`] links in their rustdoc). A submodule can only reach
//!   that name through an import that the compiler reports as unused, so moving
//!   them would have meant rewording those links; this extraction changes no
//!   doc text.
//! * `stars_for_rating` and `flag_label` are pure *display* helpers with no
//!   binding behind them, and `auto_endpoint_for_slider` /
//!   `masking_preview_for_slider` dispatch on a recipe key under a mouse
//!   gesture rather than on the keyboard. None of them is a shortcut, so
//!   folding them in would group a second concern instead of extracting one.

use crate::i18n::Str;
use eframe::egui;
use lumina_sidecar::Flag;

/// Top-level module selected in the module bar (Library / Develop / Export).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Module {
    Library,
    Develop,
    Export,
}

/// Maps a Lightroom-style module-switch keyboard shortcut to its target module.
///
/// This is a pure function so the mapping can be unit-tested without an
/// [`egui::Context`]. The mapping mirrors Lightroom's module keys:
///
/// * `G` switches to `Library` (Grid).
/// * `D` switches to `Develop`.
/// * `E` is Lightroom's "Loupe" shortcut. Lumina has no separate Loupe module,
///   so `E` is treated as an alias for `Library` (documented here so the alias
///   is intentional and not a silent fallback).
///
/// Keys that are not module shortcuts — in particular the existing `Y`
/// Before/After toggle and `Esc` eyedropper-cancel — return `None` and keep
/// their own, separate handling.
pub fn module_for_key(key: egui::Key) -> Option<Module> {
    match key {
        egui::Key::G => Some(Module::Library),
        egui::Key::D => Some(Module::Develop),
        egui::Key::E => Some(Module::Library),
        _ => None,
    }
}

/// Maps a number key to a Lightroom-style star rating (LR-01).
///
/// `1`–`5` set the rating of the active virtual copy, `0` clears it back to
/// unrated. This is a pure function so the mapping can be unit-tested without
/// an [`egui::Context`]. Note this intentionally shadows the previous zoom
/// bindings on `Num1`/`Num2` (1:1/2:1 stay reachable through the preview
/// toolbar buttons); ratings are the documented MVP priority (gap plan
/// LR-01) and sharing the keys would make one of the two a silent victim.
pub fn rating_for_key(key: egui::Key) -> Option<u8> {
    match key {
        egui::Key::Num0 => Some(0),
        egui::Key::Num1 => Some(1),
        egui::Key::Num2 => Some(2),
        egui::Key::Num3 => Some(3),
        egui::Key::Num4 => Some(4),
        egui::Key::Num5 => Some(5),
        _ => None,
    }
}

/// Maps a key to a Lightroom-style pick flag (LR-01): `P` pick, `X` reject,
/// `U` unflag. Pure function, unit-tested without an [`egui::Context`].
pub fn flag_for_key(key: egui::Key) -> Option<Flag> {
    match key {
        egui::Key::P => Some(Flag::Pick),
        egui::Key::X => Some(Flag::Reject),
        egui::Key::U => Some(Flag::Unflagged),
        _ => None,
    }
}

/// Maps a number key to a Lightroom-style color label (Welle 2, LR-17 light):
/// `6`–`9` select label `1`–`4` (red/yellow/green/blue, see
/// [`color_label_name`]), stored in the active copy's `extras["color_label"]`
/// so no sidecar schema change is needed. Pure function, unit-tested without
/// an [`egui::Context`].
pub fn color_label_for_key(key: egui::Key) -> Option<u8> {
    match key {
        egui::Key::Num6 => Some(1),
        egui::Key::Num7 => Some(2),
        egui::Key::Num8 => Some(3),
        egui::Key::Num9 => Some(4),
        _ => None,
    }
}

/// User-visible name of a color label (`0` = none). Routed through [`Str`] so
/// no panel carries a free-form literal.
pub fn color_label_name(label: u8) -> &'static str {
    match label {
        1 => Str::ColorRed.t(),
        2 => Str::ColorYellow.t(),
        3 => Str::ColorGreen.t(),
        4 => Str::ColorBlue.t(),
        _ => Str::ColorLabel.t(),
    }
}

/// Copy/paste-settings clipboard action (Welle 2, LR-09): `Cmd/Ctrl+Shift+C`
/// copies the session recipe, `Cmd/Ctrl+Shift+V` pastes it onto the active
/// virtual copy. Pure function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardAction {
    Copy,
    Paste,
}

pub fn clipboard_action_for_key(
    key: egui::Key,
    command: bool,
    shift: bool,
) -> Option<ClipboardAction> {
    if !(command && shift) {
        return None;
    }
    match key {
        egui::Key::C => Some(ClipboardAction::Copy),
        egui::Key::V => Some(ClipboardAction::Paste),
        _ => None,
    }
}

/// Display-only Develop view toggle (Welle 2): `V` black-&-white treatment
/// (recipe-backed, restores on second press), `J` clipping warnings (badge
/// computed from preview pixels), `L` lights-out (hides side panels and the
/// filmstrip, header stays). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewToggle {
    BlackWhite,
    Clipping,
    LightsOut,
}

pub fn view_toggle_for_key(key: egui::Key) -> Option<ViewToggle> {
    match key {
        egui::Key::V => Some(ViewToggle::BlackWhite),
        egui::Key::J => Some(ViewToggle::Clipping),
        egui::Key::L => Some(ViewToggle::LightsOut),
        _ => None,
    }
}

/// Panel-visibility toggle (Welle 2): `R` arms/disarms the crop mode badge
/// (edits stay in the Geometry Crop controls), `Tab` hides/shows the side
/// panels (the filmstrip stays; `L` lights-out hides that too). Pure
/// function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelToggle {
    CropMode,
    PanelsHidden,
}

pub fn panel_toggle_for_key(key: egui::Key) -> Option<PanelToggle> {
    match key {
        egui::Key::R => Some(PanelToggle::CropMode),
        egui::Key::Tab => Some(PanelToggle::PanelsHidden),
        _ => None,
    }
}

/// All-panels toggle (G-11, LRPAR-G11-OVERLAYS): `Shift+Tab` hides/shows the
/// side panels, the navigator rail AND the filmstrip (header/module bar and
/// preview stay). Plain `Tab` keeps the filmstrip (see [`panel_toggle_for_key`]);
/// the shift-aware dispatch in `update` prefers this branch. Pure function,
/// unit-tested without an [`egui::Context`].
pub fn all_panels_toggle_for_key(key: egui::Key, shift: bool) -> bool {
    matches!(key, egui::Key::Tab) && shift
}

/// Library view (G-09, LRPAR-G09-LIB): the four Library views sharing one
/// selection (`filmstrip_selection`) and one filter (`\` query + active
/// collection). `Grid` is the default thumbnail raster; `Loupe` shows the
/// active selection large; `Compare` shows the active image's Before/After
/// proxy (existing `before_after` path); `Survey` shows the multi-selection
/// side by side (falls back to the filtered raster below two selections).
/// Pure display state — never recipe/sidecar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LibraryView {
    #[default]
    Grid,
    Loupe,
    Compare,
    Survey,
    /// LRPAR-G12-FACE-20 (S5): the Library People view (cluster/person list and
    /// the confirm/split/merge actions). Display-only selection state; reached
    /// through the Library view selector (no new global shortcut is reserved —
    /// FACE-20 §3).
    People,
}

/// Maps a Lightroom-style Library view key to its view (G-09): `G` grid,
/// `E` loupe (module alias documented in [`module_for_key`]), `C` compare,
/// `N` survey. Pure function, unit-tested without an [`egui::Context`].
pub fn library_view_for_key(key: egui::Key) -> Option<LibraryView> {
    match key {
        egui::Key::G => Some(LibraryView::Grid),
        egui::Key::E => Some(LibraryView::Loupe),
        egui::Key::C => Some(LibraryView::Compare),
        egui::Key::N => Some(LibraryView::Survey),
        _ => None,
    }
}

/// Import/export module shortcut (Welle 3, LR-13 light):
/// `Cmd/Ctrl+Shift+I` jumps to Library (import lives there),
/// `Cmd/Ctrl+Shift+E` jumps to Export. The shortcuts only switch the module
/// and announce it via the status line — file dialogs and the actual export
/// stay manual. Pure function, unit-tested without an [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportExportAction {
    Import,
    Export,
}

pub fn import_export_for_key(
    key: egui::Key,
    command: bool,
    shift: bool,
) -> Option<ImportExportAction> {
    if !(command && shift) {
        return None;
    }
    match key {
        egui::Key::I => Some(ImportExportAction::Import),
        egui::Key::E => Some(ImportExportAction::Export),
        _ => None,
    }
}

/// Power-shortcut rest (G-16): plain `S` toggles the display-only softproof
/// preview. This reserves the `S` binding claimed by LRPAR-G10-VIEWER (still
/// open) instead of blocking it: the toggle is display-only, the full
/// print/gamut simulation stays G-10 follow-up work. Any modifier (Ctrl/Cmd
/// for copy-settings-adjacent chords, Alt for the `Cmd/Ctrl+Alt+S` snapshot,
/// Shift) yields `false`, so no existing chord is hijacked. Pure function,
/// unit-tested without an [`egui::Context`].
pub fn softproof_for_key(key: egui::Key, ctrl_or_command: bool, alt: bool, shift: bool) -> bool {
    matches!(key, egui::Key::S) && !ctrl_or_command && !alt && !shift
}
