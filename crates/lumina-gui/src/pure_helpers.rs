//! Pure, state-free helpers at the file edge: key resolution and badge display.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 13, file-size ratchet).
//! One responsibility: the pure value/display helpers that need no GUI state
//! — the LR-10 mask-tool key, the G-16 auto-endpoint decision and its
//! masking-preview scope (recipe keys under mouse gestures), and the Library
//! rating/flag badge display. No panel, recipe or sidecar logic; every helper
//! is unit-tested headless. The G-16 write path stays in `auto_endpoint`; the
//! keyboard shortcut table stays in `key_bindings`.

use super::*;

/// Power-shortcut rest (G-16, LRPAR-G16-POWER): `Shift`+double-click on a
/// slider label applies the auto end point of exactly that slider, reusing the
/// existing `suggest_auto_tone` path (no second algorithm). Scope decision
/// (Lightroom-conform, F-100): only `whites` (auto white point) and `blacks`
/// (auto black point) map; every other key — and any press without `Shift` or
/// without a double-click — yields `None` (callers fall back to the normal
/// single-control reset). Pure function, unit-tested without an
/// [`egui::Context`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoEndpoint {
    White,
    Black,
}

pub fn auto_endpoint_for_slider(
    key: &str,
    shift: bool,
    double_clicked: bool,
) -> Option<AutoEndpoint> {
    if !(shift && double_clicked) {
        return None;
    }
    match key {
        "whites" => Some(AutoEndpoint::White),
        "blacks" => Some(AutoEndpoint::Black),
        _ => None,
    }
}

/// Power-shortcut rest (G-16): tone-slider scope of the `Alt`+slider masking
/// preview. While `Alt` is held during a track/scroll value edit on one of
/// these Basic tone keys, the preview header shows the clipping badge through
/// the existing `J` clipping path (display-only, never recipe). Pure function,
/// unit-tested headless. Note the disambiguation: `Alt`-click on the *label*
/// stays the single-control reset (see `label_reset_requested`), `Alt`-scroll
/// stays the fine step — the preview is purely additive.
pub fn masking_preview_for_slider(key: &str) -> bool {
    matches!(
        key,
        "exposure" | "contrast" | "highlights" | "shadows" | "whites" | "blacks"
    )
}

/// Maps a key (+ Shift state) to an interactive masking tool (LR-10):
/// `K` brush, `M` linear gradient, `Shift+M` radial gradient. Pure function,
/// unit-tested without an [`egui::Context`]. Arming itself still goes through
/// [`LuminaApp::set_mask_tool`] so the geometry block stays enforced.
pub fn mask_tool_for_key(key: egui::Key, shift: bool) -> Option<MaskTool> {
    match (key, shift) {
        (egui::Key::K, _) => Some(MaskTool::Brush),
        (egui::Key::M, false) => Some(MaskTool::LinearGradient),
        (egui::Key::M, true) => Some(MaskTool::Radial),
        _ => None,
    }
}

/// Renders a star rating as a fixed-width 5-glyph badge (LR-01), e.g.
/// `3 → "★★★☆☆"`, `0 → "☆☆☆☆☆"`. Pure display helper shared by the Library
/// grid badge and the rating section; unit-tested headless.
pub fn stars_for_rating(rating: u8) -> String {
    let rating = rating.min(5) as usize;
    "★".repeat(rating) + &"☆".repeat(5 - rating)
}

/// User-visible label for a pick flag (LR-01), routed through [`Str`] so no
/// panel carries a free-form literal.
pub fn flag_label(flag: Flag) -> &'static str {
    match flag {
        Flag::Pick => Str::Pick.t(),
        Flag::Reject => Str::Reject.t(),
        Flag::Unflagged => Str::Unflagged.t(),
    }
}
