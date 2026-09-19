//! R3-OPEN-1 (Release 1.0, User-Entscheidung 2026-09-19 „Lightroom-mäßig"):
//! switching to Develop with exactly one filmstrip selection opens that image.
//!
//! Lightroom behaviour: the Develop tab opens whatever is selected in the
//! filmstrip, not only what is currently loaded. Before this module the switch
//! only changed `active_module` — a selected-but-not-loaded image stayed
//! selected while Develop kept showing the previously loaded one.
//!
//! The open goes through [`LuminaApp::open_file`] — the exact same path the
//! Library-grid double-click ([`LuminaApp::open_grid_entry_in_develop`]) uses,
//! so there is no second decode/open implementation. Every entry point into
//! Develop (`set_module` is the single funnel: module bar, `D` shortcut,
//! grid double-click, startup wiring) gets the behaviour for free.
//!
//! Guardrails (no silent state):
//! * 0 or >1 selected → behaviour unchanged, the loaded image stays.
//! * the selection already loaded (`self.path`) → no reload on a bare switch.
//! * a decode for that exact path is already in flight (the double-click
//!   selected *and* opened before the switch) → no duplicate decode.
//! * the selection points at a vanished file → [`LuminaApp::open_file`]'s
//!   existing loud error path (`show_error_banner`), never a silent ignore.
//!
//! The module-switch itself is only reacted to on an actual transition: a
//! repeat `set_module(Module::Develop)` (e.g. clicking the already-active tab)
//! re-arms the timing event but never re-opens the image.

use super::*;
use log::trace;

impl LuminaApp {
    /// R3-OPEN-1: open the single filmstrip selection when the module switch
    /// lands on Develop (see the module docs for the exact guardrails).
    pub(crate) fn open_develop_selection_on_switch(&mut self, module: Module) {
        if module != Module::Develop {
            return;
        }
        // Exactly one selected path, otherwise the loaded image stays.
        let mut selected = self.filmstrip_selection.iter();
        let Some(target) = selected.next().cloned() else {
            return;
        };
        if selected.next().is_some() {
            return;
        }
        // Already loaded: a bare switch must not reload the same file.
        if target == self.path {
            return;
        }
        // A decode for exactly this path is already running (the grid
        // double-click selected and opened before switching): reuse it.
        if self.pending_load_path.as_deref() == Some(target.as_str()) {
            return;
        }
        trace!("GUI interaction: develop switch opens selection {target}");
        // Same open path as the grid double-click — no duplicate code.
        self.open_file(target);
    }
}
