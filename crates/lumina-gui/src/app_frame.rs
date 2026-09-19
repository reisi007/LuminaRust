//! GUI-REFACTOR-W2-20 S2.7: the top-level app-frame pieces, extracted
//! verbatim from `lib.rs`.
//!
//! [`LuminaApp::draw_preview_area`] is the central preview pane (view toolbar,
//! zoom toolbar and the routing-fallback badge), [`LuminaApp::draw_module_bar`]
//! the top module bar, [`LuminaApp::draw_view_toolbar`]/[`LuminaApp::zoom_toolbar`]
//! the overlay toolbars and [`LuminaApp::wants_wheel_zoom`]/[`LuminaApp::zoom_label`]
//! the wheel-zoom and label helpers. The `eframe::App::ui` frame itself stays at
//! the crate root (see the module docs there): its body exceeds the strict
//! 500-line rule for new files, so it is not moved whole. No behaviour changes:
//! every panel, toggle and label is byte-identical. The externally called
//! helpers are `pub(crate)`; the toolbar painters stay private.

use super::*;
use log::trace;

impl LuminaApp {
    /// Toggle the Spot-Heal tool exactly like the `Q` shortcut: one status flip
    /// per activation, shared by the toolbar button and the keyboard so both
    /// can never diverge. Pure UI state ([`Self::set_spot_tool`]); the
    /// recipe/sidecar is never touched. (The Dust-Removal panel button arms
    /// the same tool state via `set_spot_tool` directly, without the status
    /// line — only toolbar and `Q` share the status flip.)
    pub(crate) fn toggle_spot_heal_tool(&mut self) {
        let next = if self.spot_tool == SpotTool::None {
            SpotTool::Heal
        } else {
            SpotTool::None
        };
        self.set_spot_tool(next);
        self.status = if next == SpotTool::Heal {
            "Spot heal armed (Q)".into()
        } else {
            "Spot heal disarmed".into()
        };
    }

    /// Central working area: a zoom toolbar (Lightroom-like Fit / 1:1 / 200% /
    /// Fit Width + a live zoom readout and a collapsed-navigator reopen button),
    /// then the rendered preview and the render-state label. Shared by the
    /// Develop and Export modules.
    pub(crate) fn draw_preview_area(&mut self, ctx: &egui::Context, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            {
                if !self.navigator_open {
                    if ui.button(Str::Navigator.t()).clicked() {
                        self.navigator_open = true;
                        trace!("GUI interaction: navigator open");
                    }
                    ui.separator();
                }
            }
            ui.label(Str::Preview.t());
            ui.separator();
            self.zoom_toolbar(ui);
        });
        // F-100 Klickbarkeit (GUI-CLICK-ALL-17): the view toggles that used to
        // be keyboard-only get a clickable button row under the zoom toolbar.
        self.draw_view_toolbar(ui);
        self.update_texture(ctx);
        self.draw_preview(ui);
        // UX-SLICE-1 (UXG-07): the render hash moved to the app status line
        // (header, see `draw_status_line`); the canvas edge now carries only
        // the color-coded state badges. The states themselves are unchanged:
        // an in-flight draft render is "Draft", a missing `render_key` is
        // "Stale"/pending (never a silent fallback).
        if self.preview_is_draft {
            ui.colored_label(RENDER_STATE_DRAFT_COLOR, Str::Draft.t());
        }
        if self.render_key.is_none() {
            ui.colored_label(RENDER_STATE_STALE_COLOR, Str::RenderStateStale.t());
        }
        // Welle 2 view-state badges: crop mode (`R`), B&W treatment (`V`)
        // and the clipping overlay (`J`) advertise their state in the
        // preview header so no toggle is ever silent.
        if self.crop_mode {
            ui.colored_label(egui::Color32::YELLOW, Str::CropModeOn.t());
        }
        if self.bw_active() {
            ui.label(Str::BlackWhiteOn.t());
        }
        if self.clipping_effective() {
            match self.clipping_detail() {
                Some((shadow, highlight)) => {
                    let text = Str::ClippingDetailPattern
                        .t()
                        .replacen("{}", &format!("{:.1}", shadow * 100.0), 1)
                        .replacen("{}", &format!("{:.1}", highlight * 100.0), 1);
                    ui.colored_label(egui::Color32::YELLOW, text);
                }
                None => {
                    ui.colored_label(egui::Color32::YELLOW, Str::ClippingOn.t());
                }
            }
        }
        // G-16: the `S` softproof preview advertises its state in the preview
        // header so the toggle is never silent (display-only, never recipe).
        if self.softproof_preview {
            ui.colored_label(egui::Color32::YELLOW, Str::SoftproofOn.t());
        }
        // R2-GUIMOD-06: surface the otherwise-silent GPU→CPU routing fallback
        // as a visible status badge (with tooltip) instead of only a stderr
        // `log::warn!`. No-op while `gpu_route_fallback` is `None` (GPU present
        // path usable, or no GPU context bound at all).
        #[cfg(feature = "gpu")]
        self.draw_routing_fallback_badge(ui);
    }

    /// GUI-LENSFUN-GATE-3 (F2): paint the GPU→CPU routing fallback badge for
    /// the frame painted last. The reason list can outgrow the (narrow) panel
    /// edge; the badge truncates with an ellipsis instead of clipping its tail
    /// at the panel boundary, while the full reason stays available in the
    /// tooltip. Extracted from [`Self::draw_preview_area`] so headless layout
    /// tests can pin the no-overflow contract without a bound adapter.
    #[cfg(feature = "gpu")]
    pub(crate) fn draw_routing_fallback_badge(&self, ui: &mut egui::Ui) {
        if let Some(reason) = &self.gpu_route_fallback {
            ui.add(
                egui::Label::new(egui::RichText::new(reason).color(egui::Color32::YELLOW))
                    .truncate(),
            )
            .on_hover_text(Str::CpuFallbackTooltip.t().to_string());
        }
    }

    /// Top module bar (Library / Develop / Export) plus the Before/After
    /// toggle, extracted from the app layout so the headless F-100 shortcut →
    /// button audit can paint it without an `eframe::Frame`. The module labels
    /// advertise their Lightroom shortcuts (`G`, `D`).
    pub(crate) fn draw_module_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            for (module, label) in [
                (Module::Library, Str::LibraryShortcut.format_arg("G")),
                (Module::Develop, Str::DevelopShortcut.format_arg("D")),
                (Module::Export, Str::Export.t().to_string()),
            ] {
                if ui
                    .selectable_label(self.active_module == module, label)
                    .clicked()
                {
                    self.set_module(module);
                }
            }
            ui.separator();
            if ui.button(Str::BeforeAfter.t()).clicked() {
                self.toggle_before_after();
            }
        });
    }

    /// UX-LOOK-TOOLBAR-18 (UXG-04): the preview tool strip at the LR place
    /// (under the histogram / over the image), now icon-based. It carries the
    /// interactive develop tools (Crop / Heal / Red-Eye / Masking) *and* the
    /// display-only view toggles that used to be keyboard-only (`PanelToggle`
    /// crop/panels, `ViewToggle` clipping/lights-out, split, fullscreen).
    ///
    /// Every button routes through the same `toggle_*`/`set_*` path as its
    /// keyboard shortcut — one status/`info!` per flip, no recipe or sidecar
    /// semantics, so button and keyboard can never diverge. Icons are painted
    /// by [`crate::icon_toolbar`]; the tooltips carry the existing working
    /// labels and shortcuts (Namen-Vorbehalt: no new final wording).
    fn draw_view_toolbar(&mut self, ui: &mut egui::Ui) {
        use crate::icon_toolbar::{icon_button, ToolbarIcon};
        ui.horizontal_wrapped(|ui| {
            // Interactive develop tools (LR order).
            if icon_button(ui, ToolbarIcon::Crop, self.crop_mode).clicked() {
                self.toggle_crop_mode();
            }
            let heal_active = self.spot_tool != SpotTool::None;
            if icon_button(ui, ToolbarIcon::Heal, heal_active).clicked() {
                self.toggle_spot_heal_tool();
            }
            if icon_button(ui, ToolbarIcon::RedEye, self.red_eye_pick_mode).clicked() {
                self.set_red_eye_pick_mode(!self.red_eye_pick_mode);
            }
            let mask_active = self.mask_tool != MaskTool::None;
            if icon_button(ui, ToolbarIcon::Masking, mask_active).clicked() {
                self.set_mask_tool(if mask_active {
                    MaskTool::None
                } else {
                    MaskTool::Brush
                });
            }
            ui.separator();
            // Display-only view toggles.
            if icon_button(ui, ToolbarIcon::Clipping, self.clipping_overlay).clicked() {
                self.toggle_clipping_overlay();
            }
            if icon_button(ui, ToolbarIcon::Split, self.before_after_split).clicked() {
                self.toggle_split_view();
            }
            if icon_button(ui, ToolbarIcon::LightsOut, self.lights_out).clicked() {
                self.toggle_lights_out();
            }
            if icon_button(ui, ToolbarIcon::Panels, self.panels_hidden).clicked() {
                self.toggle_panels_hidden();
            }
            if icon_button(ui, ToolbarIcon::AllPanels, self.all_panels_hidden).clicked() {
                self.toggle_all_panels_hidden();
            }
            if icon_button(ui, ToolbarIcon::Fullscreen, self.fullscreen).clicked() {
                self.toggle_fullscreen();
            }
        });
    }

    /// Lightroom-like zoom toolbar: absolute zoom modes (re-derived each frame
    /// from the pane) plus a live zoom percentage readout. The active mode is
    /// highlighted. Rendered by the preview-area header.
    fn zoom_toolbar(&mut self, ui: &mut egui::Ui) {
        // GUI-PREVIEW-NAV-1 (F-100): the readout names the nominal step, never
        // the effective on-screen scale.
        ui.label(format!("{}: {}", Str::Zoom.t(), self.zoom_label()));
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::Fit, Str::ZoomFit.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::Fit);
        }
        for (mode, label) in [
            (ZoomMode::Quarter, Str::Zoom25),
            (ZoomMode::Half, Str::Zoom50),
            (ZoomMode::ThreeQuarter, Str::Zoom75),
        ] {
            if ui
                .selectable_label(self.zoom_mode == mode, label.t())
                .clicked()
            {
                self.set_zoom_mode(mode);
            }
        }
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::OneToOne, Str::ZoomOneToOne.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::OneToOne);
        }
        if ui
            .selectable_label(
                self.zoom_mode == ZoomMode::TwoHundred,
                Str::ZoomTwoHundred.t(),
            )
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::TwoHundred);
        }
        if ui
            .selectable_label(self.zoom_mode == ZoomMode::FitWidth, Str::ZoomFitWidth.t())
            .clicked()
        {
            self.set_zoom_mode(ZoomMode::FitWidth);
        }
    }

    /// Whether a mouse-wheel event over the preview zooms (GUI-PREVIEW-NAV-1):
    /// only while Ctrl (or Cmd on macOS) is held. Without a modifier the wheel
    /// scrolls/pans and must never switch the zoom to `Custom`.
    pub(crate) fn wants_wheel_zoom(modifiers: &egui::Modifiers) -> bool {
        modifiers.ctrl || modifiers.command
    }

    /// Nominal zoom step for the toolbar readout (GUI-PREVIEW-NAV-1, F-100):
    /// absolute modes name their nominal step (Fit/25/50/75/100/200 %,
    /// Fit-Breite); `Custom` — the pinned zoom+pan view — names itself. The
    /// effective on-screen scale is deliberately not shown (F-100: höchstens
    /// Tooltip). Pure helper, unit-tested headless.
    pub(crate) fn zoom_label(&self) -> String {
        match self.zoom_mode {
            ZoomMode::Fit => Str::ZoomFit.t().to_string(),
            ZoomMode::Quarter => Str::Zoom25.t().to_string(),
            ZoomMode::Half => Str::Zoom50.t().to_string(),
            ZoomMode::ThreeQuarter => Str::Zoom75.t().to_string(),
            ZoomMode::OneToOne => Str::Zoom100.t().to_string(),
            ZoomMode::TwoHundred => Str::ZoomTwoHundred.t().to_string(),
            ZoomMode::FitWidth => Str::ZoomFitWidth.t().to_string(),
            ZoomMode::Custom => Str::ZoomCustom.t().to_string(),
        }
    }
}
