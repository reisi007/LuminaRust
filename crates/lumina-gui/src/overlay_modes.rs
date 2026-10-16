//! G-11 overlay modes: overlay/pin visibility enums and their Str-routed names.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 4, file-size ratchet).
//! Pure value types and name lookups; no GUI behaviour change.

use super::*;

/// Tool-overlay mode (G-11): how the mask-matte overlay behaves for the
/// masking/retouch tools. Global across tools (one predictable switch, F-100
/// G-11 SOLL). `Always` is the historical behaviour (overlay whenever a prompt
/// exists) and therefore the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OverlayMode {
    #[default]
    Always,
    Auto,
    Never,
}

/// Edit-pin visibility (G-11): whether numbered edit pins (mask anchors + spot
/// centres) are painted. Default `Auto` (pins only while a tool is armed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PinVisibility {
    Always,
    #[default]
    Auto,
    Never,
}

/// User-visible name of an [`OverlayMode`], routed through [`Str`] so no panel
/// carries a free-form literal.
pub fn overlay_mode_name(mode: OverlayMode) -> &'static str {
    match mode {
        OverlayMode::Always => Str::OverlayAlways.t(),
        OverlayMode::Auto => Str::OverlayAuto.t(),
        OverlayMode::Never => Str::OverlayNever.t(),
    }
}

/// User-visible name of a [`PinVisibility`], routed through [`Str`].
pub fn pin_visibility_name(visibility: PinVisibility) -> &'static str {
    match visibility {
        PinVisibility::Always => Str::OverlayAlways.t(),
        PinVisibility::Auto => Str::OverlayAuto.t(),
        PinVisibility::Never => Str::OverlayNever.t(),
    }
}

/// User-visible name of an [`AiSelectKind`] (G-03), routed through [`Str`].
/// Covers every variant (DoD §3: no sampling over the class).
pub fn ai_select_kind_name(kind: AiSelectKind) -> &'static str {
    match kind {
        AiSelectKind::Subject => Str::AiSubject.t(),
        AiSelectKind::Sky => Str::AiSky.t(),
        AiSelectKind::Background => Str::AiBackground.t(),
        AiSelectKind::Objects => Str::AiObjects.t(),
        AiSelectKind::People => Str::AiPeople.t(),
    }
}
