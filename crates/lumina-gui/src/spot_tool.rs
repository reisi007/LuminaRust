//! R5-DUST-23: interactive Spot-Heal tool (dab + live-size cursor + size
//! shortcuts) and its toolbar-near option strip.
//!
//! The Dust-Removal arming moved out of the sidebar into the preview tool
//! strip (`ToolbarIcon::Heal`, `Q` as alias, see `app_frame`/`icon_toolbar`);
//! the option controls are painted by
//! [`LuminaApp::draw_spot_tool_options`](crate::develop_heal). This module owns
//! the pointer interaction that used to be missing entirely: without it the
//! armed tool did nothing on the image ("Dust Removal funktioniert nicht").
//!
//! Everything here is session state plus the existing
//! [`LuminaApp::commit_spot_heal`](crate::LuminaApp::commit_spot_heal) recipe
//! path — no second render path, no silent fallback.

use super::*;
use log::info;

/// The `[` / `]` size-shortcut mapping (Lightroom-like brush size). Pure and
/// exhaustive so the shortcut audit can prove no other bound key lands here.
pub(crate) fn spot_size_factor_for_key(key: egui::Key) -> Option<f32> {
    match key {
        egui::Key::OpenBracket => Some(1.0 / 1.1),
        egui::Key::CloseBracket => Some(1.1),
        _ => None,
    }
}

impl LuminaApp {
    /// `[`/`]` alias for the armed spot tool: shrink/grow the dab size. Ignored
    /// while a widget wants keyboard input (never hijacks typing). Visible and
    /// logged (`info!`, DoD §4).
    pub(crate) fn handle_spot_size_shortcuts(&mut self, ctx: &egui::Context) {
        if self.spot_tool == SpotTool::None || ctx.egui_wants_keyboard_input() {
            return;
        }
        for key in [egui::Key::OpenBracket, egui::Key::CloseBracket] {
            if ctx.input(|i| i.key_pressed(key)) {
                if let Some(factor) = spot_size_factor_for_key(key) {
                    self.nudge_spot_radius(factor);
                }
            }
        }
    }

    /// Scale the spot dab radius by `factor` (clamped to the validated
    /// `1..=512` source-pixel range). No-op when already clamped; otherwise the
    /// shared [`Self::set_spot_radius`] path runs (recipe-dirty commit) and the
    /// change is logged.
    pub(crate) fn nudge_spot_radius(&mut self, factor: f32) {
        let next = (self.spot_radius * factor).clamp(1.0, 512.0);
        if (next - self.spot_radius).abs() <= f32::EPSILON {
            return;
        }
        self.set_spot_radius(next);
        info!("GUI interaction: spot size -> {next:.0} px");
    }

    /// Deterministic auto-clone source offset for a dab at `nx`: sample
    /// `1.5 * radius` source pixels toward the image interior (so the source
    /// stays in bounds and clears the dust) — horizontally only, matching the
    /// pipeline's `source_offset` semantics (`source-normalized`).
    pub(crate) fn spot_auto_offset(&self, nx: f32) -> lumina_sidecar::Point2 {
        let (width, _height) = self.image_dims().unwrap_or((1, 1));
        let offset_px = (self.spot_radius * 1.5).max(1.0);
        let direction = if nx <= 0.5 { 1.0 } else { -1.0 };
        lumina_sidecar::Point2 {
            x: (direction * offset_px / width.max(1) as f32).clamp(-1.0, 1.0),
            y: 0.0,
        }
    }

    /// Commit one spot-heal dab at the normalized source point with the current
    /// size/feather/opacity tool settings. Loud on validation errors (never a
    /// silent no-op).
    pub(crate) fn spot_dab(&mut self, nx: f32, ny: f32) -> Result<(), GuiError> {
        let offset = self.spot_auto_offset(nx);
        info!(
            "GUI interaction: spot dab at ({nx:.3},{ny:.3}) radius {:.0}px",
            self.spot_radius
        );
        self.commit_spot_heal(
            lumina_sidecar::Point2 { x: nx, y: ny },
            self.spot_radius,
            self.spot_feather,
            offset,
            self.spot_opacity,
        )
    }

    /// Drive the armed spot tool on the preview widget: one dab per click plus
    /// the live-size circle cursor while hovering (`radius * view scale` screen
    /// points). A disarmed tool is a hard no-op.
    pub(crate) fn handle_spot_tool_interaction(
        &mut self,
        ui: &egui::Ui,
        response: &egui::Response,
        rect: egui::Rect,
        scale: f32,
    ) {
        if self.spot_tool == SpotTool::None {
            return;
        }
        if let Some(pos) = response.hover_pos() {
            let radius = (self.spot_radius * scale).clamp(2.0, 4000.0);
            ui.painter().circle_stroke(
                pos,
                radius,
                egui::Stroke::new(1.5_f32, crate::theme::ACCENT),
            );
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let full = self.image_dims().unwrap_or((1, 1));
                let roi = self
                    .preview_roi
                    .map(|r| Self::roi_in_full_pixels(r, full.0, full.1, self.preview_render_src));
                let (nx, ny) = Self::to_normalized(pos, rect, roi, full);
                if let Err(error) = self.spot_dab(nx, ny) {
                    self.show_error(error);
                }
            }
        }
    }
}
