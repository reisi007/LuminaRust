//! The working view's chrome: which frame parts are hidden and why.
//!
//! The side panels, the navigator rail and the bottom filmstrip form the
//! chrome around the preview. Three session-only flags hide parts of it
//! (`L` lights-out, `Tab` panels-hide, `Shift+Tab` all-panels-hide) and the
//! draw paths ask the same predicates which of it is visible right now.
//! One responsibility: the chrome-visibility policy of the working view —
//! the flags that hide the frame and the single answer per draw gate which
//! part of it is still shown.
//!
//! The fullscreen working view (`toggle_fullscreen`/`set_fullscreen`/
//! `is_fullscreen`) keeps its own responsibility in `lib.rs`: it settles the
//! zoom on Fit, which is a view-mode concern and not a chrome concern, so it
//! is read here only as the `fullscreen` flag. Nothing is painted here
//! either: the draw gates that ask these predicates stay in the root layout
//! of `lib.rs`, the chrome painters in `navigator.rs` and `filmstrip_frame.rs`
//! (`app_frame.rs` paints what stays visible); the module and view enums are
//! in `key_bindings.rs` — none of them is re-implemented here.
//!
//! Not the same thing as its siblings: `library_views.rs` holds the
//! library's loupe/compare/survey *views*, `overlay_visibility.rs`
//! the *preview's* overlay chrome (overlay mode, edit pins) and
//! `mask_visibility.rs` the mask-view gate. This module is the chrome of
//! the *working view* — the panels, the navigator rail and the filmstrip
//! around the preview.
//!
//! Extracted verbatim for the `GUI-LIB-SPLIT-65` file-size ratchet; no
//! behaviour change and no visibility change (all eight methods stay `pub`).

use super::*;

impl LuminaApp {
    /// Toggle lights-out (`L`, Welle 2). Display-only: hides the side panels
    /// and the filmstrip; header, module bar and preview stay so status and
    /// errors remain visible. Never mutates the recipe.
    pub fn toggle_lights_out(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleLightsOut);
        self.lights_out = !self.lights_out;
        info!("GUI interaction: toggle_lights_out -> {}", self.lights_out);
        self.status = if self.lights_out {
            Str::LightsOutOn.t().into()
        } else {
            Str::LightsOutOff.t().into()
        };
    }

    /// Toggle side-panel visibility (`Tab`, Welle 2). Display-only: hides the
    /// left/right panels; the filmstrip stays (unlike `L` lights-out).
    /// Never mutates the recipe.
    pub fn toggle_panels_hidden(&mut self) {
        instrument_gui_action!(self, GuiAction::TogglePanelsHidden);
        self.panels_hidden = !self.panels_hidden;
        info!(
            "GUI interaction: toggle_panels_hidden -> {}",
            self.panels_hidden
        );
        self.status = if self.panels_hidden {
            Str::PanelsHiddenOn.t().into()
        } else {
            Str::PanelsHiddenOff.t().into()
        };
    }

    /// Toggle all panels (`Shift+Tab`, G-11). Display-only: hides the side
    /// panels, the navigator rail and the filmstrip; header/module bar and
    /// preview stay so status and errors remain visible. Never mutates the
    /// recipe or the sidecar (session-only like `Tab`).
    pub fn toggle_all_panels_hidden(&mut self) {
        instrument_gui_action!(self, GuiAction::ToggleAllPanelsHidden);
        self.all_panels_hidden = !self.all_panels_hidden;
        log::info!(
            "GUI interaction: toggle_all_panels_hidden -> {}",
            self.all_panels_hidden
        );
        self.status = if self.all_panels_hidden {
            Str::AllPanelsHiddenOn.t().into()
        } else {
            Str::AllPanelsHiddenOff.t().into()
        };
    }

    /// Whether `Shift+Tab` all-panels-hide is armed (read-only accessor for
    /// headless tests).
    pub fn all_panels_hidden(&self) -> bool {
        self.all_panels_hidden
    }

    /// Whether the plain `Tab` side-panel hide is armed (read-only accessor
    /// for the R5-MASKVIS-25 preview-growth test).
    pub fn panels_hidden(&self) -> bool {
        self.panels_hidden
    }

    /// Whether any side chrome is hidden: plain `Tab` panels-hide, `L`
    /// lights-out, `F` fullscreen or `Shift+Tab` all-panels-hide. Shared by
    /// the side-panel and navigator draw gates; [`Self::chrome_hidden`] keeps
    /// its historical meaning (without the G-11 flag) for compatibility.
    pub fn side_chrome_hidden(&self) -> bool {
        self.chrome_hidden() || self.all_panels_hidden
    }

    /// Whether side chrome (panels, navigator, filmstrip) is hidden: `Tab`
    /// panels-hide, `L` lights-out or `F` fullscreen (Welle 3). Shared by the
    /// draw paths so fullscreen hides exactly the lights-out chrome — and,
    /// with `fullscreen == false`, every condition evaluates exactly as
    /// before (no default-layout pixel change).
    pub fn chrome_hidden(&self) -> bool {
        self.panels_hidden || self.lights_out || self.fullscreen
    }

    /// Whether the bottom filmstrip is drawn for the current module state
    /// (F-100: visible in Library, Develop AND Export; `Tab` panels-hide
    /// keeps it, `L` lights-out and `F` fullscreen hide it). Single source
    /// of truth shared by the draw path and the headless regression tests
    /// (GUI-VISION-1: Export deliberately shows the filmstrip — there is no
    /// Export-specific reason to hide it, the old
    /// `Library | Develop`-only gate was an oversight against the F-100 norm
    /// "Der Filmstreifen ist in allen drei Modulen sichtbar").
    pub fn shows_filmstrip(&self) -> bool {
        matches!(
            self.active_module,
            Module::Library | Module::Develop | Module::Export
        ) && !self.lights_out
            && !self.fullscreen
            && !self.all_panels_hidden
    }
}
