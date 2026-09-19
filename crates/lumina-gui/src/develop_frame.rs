//! GUI-REFACTOR-W2-20 S2.4: the Develop frame (section order table, panel
//! shell and scroll content), extracted verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_develop_panel`] is the bottom-up panel shell with the
//! pinned global-action footer; [`LuminaApp::DEVELOP_SECTIONS`] is the
//! normative F-100 section order and [`LuminaApp::develop_scroll_content`] the
//! top-down scroll content (histogram, rating, the eight sections,
//! generative/heal and the load controls). UX-LOOK-LAYOUT-18 moved the
//! Presets/Snapshots/History panels to the left rail
//! (`develop_left_rail.rs`); the right-panel section order and the footer
//! actions are unchanged. `draw_develop_panel` is `pub(crate)` (the app root
//! and headless tests call it); the private scroll helper stays in this module.

use super::*;

impl LuminaApp {
    /// Normative draw order of the eight F-100 Develop sections, rendered by
    /// [`LuminaApp::draw_develop_panel`] in exactly this sequence.
    /// Lightroom Classic panel order: Basic → Tone Curve → HSL/Color →
    /// Color Grading → **Detail → Effects** → Optics → Geometry → Masking.
    ///
    /// F-103-N10 (user decision 2026-08-25): **Detail BEFORE Effects** —
    /// Sharpening/Noise Reduction are shown above Vignette/Grain, matching
    /// Lightroom Classic (previously Effects was drawn first; SOLL and GUI
    /// were aligned in `feature/platform/cli-gui-wasm.md` § UI-Konventionen).
    ///
    /// Collapse state is keyed by the section label (egui auto-IDs), not by
    /// position, so reordering here neither changes nor resets user collapse
    /// state. The order is pinned by the
    /// `develop_section_order_is_lightroom_conform` test below.
    pub(crate) const DEVELOP_SECTIONS: &[(Str, DevelopSectionDraw)] = &[
        (Str::Basic, LuminaApp::draw_basic),
        (Str::ToneCurve, LuminaApp::draw_tone_curve),
        (Str::Color, LuminaApp::draw_color),
        (Str::Detail, LuminaApp::draw_detail),
        (Str::Effects, LuminaApp::draw_effects),
        (Str::Optics, LuminaApp::draw_optics),
        (Str::Geometry, LuminaApp::draw_geometry),
        (Str::Masking, LuminaApp::draw_masking),
    ];

    /// The full Develop control stack: the eight F-100 sections in fixed order,
    /// then the footer admin actions. Presets/History/Snapshots live in the
    /// left rail (UX-LOOK-LAYOUT-18), not in this panel. Every
    /// adjustment uses [`lr_slider`] so the F-100 reset/scroll/scale rules apply.
    ///
    /// GUI-VISION-1: the outer layout is bottom-up so the global actions form
    /// a pinned footer at the panel bottom edge — never half-cut below a
    /// scroll fold (kittest `develop_basic`/`histogram_graphic` goldens).
    /// Footer rows, edge-first: commit row (Save Recipe / Sidecar, Render /
    /// Apply), then maintenance row (Reset, Match Total Exposure,
    /// Regenerate Stale / Missing), then the Reset-Sliders checkbox on top.
    /// Code order is bottom-first (the first row added lands lowest); the
    /// scroll content itself is explicitly top-down again because `ScrollArea`
    /// inherits the parent layout (`Ui::new_child` falls back to
    /// `*self.layout()`).
    pub(crate) fn draw_develop_panel(&mut self, ui: &mut egui::Ui) {
        ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
            ui.add_space(2.0);
            // UX-LOOK-LAYOUT-18: the admin actions are spread into readable
            // rows instead of a cramped one-per-line stack. The commit row
            // (Save / Render) sits closest to the panel edge; the global
            // maintenance row (Reset / Match / Regenerate) is grouped above it.
            // Bottom-up insertion: the first row added lands lowest.
            ui.horizontal_wrapped(|ui| {
                if ui.button(Str::SaveRecipe.t()).clicked() {
                    self.save_recipe_action();
                }
                if ui.button(Str::RenderApply.t()).clicked() {
                    self.render_action();
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button(Str::Reset.t()).clicked() {
                    self.reset();
                }
                if ui.button(Str::MatchExposure.t()).clicked() {
                    if let Err(error) = self.match_total_exposure(0.5) {
                        self.show_error(error);
                    }
                }
                // GUI-GEN-GRANULAR-10 (F-100): the collective default — regenerate
                // every stale/missing AI/analysis value (masks, auto-tone,
                // matching) and skip the fresh ones. Explicit only, never implicit.
                if ui.button(Str::RegenerateStale.t()).clicked() {
                    match self.regenerate_stale() {
                        Ok(done) if done.is_empty() => {
                            self.status = Str::NothingStale.t().to_string();
                        }
                        Ok(done) => {
                            self.status = Str::RegeneratedStale.format_arg(&done.join(", "));
                        }
                        Err(error) => self.show_error(error),
                    }
                }
            });
            // LRPAR-G01-BASIC: "Reset Sliders Automatically" — folder-inherited
            // edit behaviour (see `set_reset_sliders_automatically`).
            let mut reset_auto = self.reset_sliders_automatically;
            ui.checkbox(&mut reset_auto, Str::ResetSlidersAutomatically.t());
            if reset_auto != self.reset_sliders_automatically {
                self.set_reset_sliders_automatically(reset_auto);
            }
            ui.separator();
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                        self.develop_scroll_content(ui);
                    });
                });
        });
    }

    /// Scrolling part of the Develop panel (sections + load controls); the
    /// pinned action footer lives in [`LuminaApp::draw_develop_panel`].
    fn develop_scroll_content(&mut self, ui: &mut egui::Ui) {
        // GUI-HISTOGRAM-1: the histogram is its own collapsible
        // section (default open) at the top of the Develop panel —
        // never in the module bar.
        self.draw_histogram_section(ui);
        ui.separator();
        // GUI-RIGHT-THUMB-1: no panel thumbnail here. The removed
        // `draw_crop_thumb` duplicated the main preview a third time (left
        // rail overview + bottom filmstrip + right panel) and showed the
        // possibly ROI-cropped preview texture as if it were the full frame
        // when zoomed.
        //
        // UX-LOOK-LAYOUT-18: Presets/Snapshots/History moved to the left rail
        // (`draw_develop_left_rail`); the right panel now follows Lightroom
        // Classic with the histogram, the rating row and the eight adjustment
        // sections.
        self.draw_rating_section(ui);
        ui.separator();
        // The eight adjustment sections are grayed and non-interactive until an
        // image is loaded (F-100 disabled-while-empty behaviour).
        ui.add_enabled_ui(self.original.is_some(), |ui| {
            // F-100 section order (incl. F-103-N10: Detail BEFORE
            // Effects) has its single source of truth in
            // `DEVELOP_SECTIONS`; see there.
            for (_, draw_section) in Self::DEVELOP_SECTIONS {
                draw_section(self, ui);
            }
            self.draw_generative_expand(ui);
            self.draw_spot_heal(ui);
        });
        ui.separator();
        // GUI-VISION-1 (same bug class as the Export Choose row):
        // button-first (right-to-left) so Load stays inside the panel.
        let mut load_clicked = false;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            load_clicked = ui.button(Str::Load.t()).clicked();
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.text_edit_singleline(&mut self.path);
            });
        });
        if load_clicked {
            let path = self.path.clone();
            self.begin_load_path(path);
        }
        if ui.button(Str::ChooseFile.t()).clicked() {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                // REVIEW-GUI-PATHDESYNC-1: no immediate
                // `self.path` commit; `finish_decode` adopts the
                // path after a successful decode.
                self.begin_load_path(path.display().to_string());
            }
        }
    }
}
