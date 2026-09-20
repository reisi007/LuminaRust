//! UX-LOOK-LAYOUT-18 (Release 1.0): the Develop left rail — Lightroom-Classic
//! macro layout.
//!
//! The left rail hosts, top to bottom, the **Navigator**, the **Presets**
//! panel, the **Snapshots** panel and the **History** panel (whose action row
//! carries the Copy/Paste/Duplicate/Stack/Snapshot buttons, F-100). It is a
//! pure layout composition: every panel is the existing painter —
//! [`LuminaApp::draw_navigator`] for the navigator, `draw_presets_section`,
//! `draw_snapshots_section` and `draw_history_section` for the rest — so no
//! recipe/sidecar behaviour is duplicated or changed. The right panel keeps the
//! histogram, the rating section, the eight F-100 adjustment sections and the
//! pinned footer ([`LuminaApp::draw_develop_panel`]).
//!
//! **Namen-Vorbehalt (User-Regel 2026-09-19):** the section labels stay the
//! existing `Str::*` working labels; the final naming round (NAMING-F1)
//! decides the visible terms. No new `Str` variant was introduced here because
//! `i18n.rs` is under the file-size ratchet (> 500 lines must not grow); the
//! Snapshots header therefore reuses `Str::SnapshotButton` as a working label.
//!
//! The `Export` module keeps the plain navigator panel; only `Develop` gets the
//! full left rail (see [`LuminaApp::draw_left_rail_panel`]). R4-UX-1
//! (2026-09-20) removed the duplicate thumbnail rail from the navigator in both
//! modules — the bottom filmstrip stays the single selection surface.

use super::*;

impl LuminaApp {
    /// Left edge panel dispatcher: `Develop` gets the full Lightroom-Classic
    /// left rail (Navigator + Presets + Snapshots + History), every other
    /// module keeps the plain Navigator panel (R4-UX-1: no duplicate thumbnail
    /// rail). The caller ([`eframe::App::ui`]) keeps the
    /// `navigator_open`/module/chrome gate, so `Tab`/`Shift+Tab`/`L`/`F` and the
    /// Library module stay untouched.
    pub(crate) fn draw_left_rail_panel(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let develop = self.active_module == Module::Develop;
        // Distinct panel ids: `egui` remembers a resizable panel's size per id,
        // so a shared id would let the first frame's module (the default start
        // is Develop) impose the wider rail width on the Export navigator. The
        // rail carries text-heavy panels (presets, history), so Develop opens
        // wider than the thumbnail-only navigator rail; both stay resizable.
        let (id, default_size) = if develop {
            ("develop_rail", 240.0)
        } else {
            ("navigator", 150.0)
        };
        egui::Panel::left(id)
            .resizable(true)
            .default_size(default_size)
            .show(ui, |ui| {
                if develop {
                    self.draw_develop_left_rail(ctx, ui);
                } else {
                    self.draw_navigator(ctx, ui);
                }
            });
    }

    /// Develop left rail (UX-LOOK-LAYOUT-18): the navigator on top, then the
    /// Presets/Snapshots/History panels in a single vertical scroll area.
    ///
    /// The navigator gets a bounded slice of the rail height so its own
    /// thumbnail `ScrollArea` cannot claim the whole column and push the panels
    /// below the fold (nested scroll areas: the inner `show_rows` would else
    /// size against the full remaining height).
    pub(crate) fn draw_develop_left_rail(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        let rail_height = ui.available_height();
        let navigator_height = (rail_height * 0.5).clamp(200.0, 460.0);
        ui.allocate_ui(egui::vec2(ui.available_width(), navigator_height), |ui| {
            self.draw_navigator(ctx, ui)
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.draw_presets_section(ui);
                self.draw_snapshots_section(ui);
                self.draw_history_section(ui);
            });
    }

    /// Snapshots panel (UX-LOOK-LAYOUT-18): the active virtual copy's named
    /// snapshots, restore-on-click through the existing
    /// [`LuminaApp::restore_snapshot`] path. Creating a snapshot stays on the
    /// documented `Cmd/Ctrl+Alt+S` shortcut and the History action-row button
    /// (single F-100 action surface, no duplicated handler). Read-only against
    /// the document; no schema or sidecar change.
    pub(crate) fn draw_snapshots_section(&mut self, ui: &mut egui::Ui) {
        ui.collapsing(Str::SnapshotButton.t(), |ui| {
            let entries = self.snapshots();
            if entries.is_empty() {
                ui.label(Str::NoHistory.t());
                return;
            }
            for (id, name) in entries {
                if ui.selectable_label(false, &name).clicked() {
                    if let Err(error) = self.restore_snapshot(&id) {
                        self.show_error(error);
                    }
                }
            }
        });
    }
}
