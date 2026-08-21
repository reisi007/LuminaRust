//! Lightroom-dark-inspired egui style.
//!
//! Charcoal panels, a near-black navigator/working area, thin separators, a
//! single subtle accent for selection/activation and compact spacing.  This is a
//! deliberate Lumina interpretation of the Lightroom *feeling*, not a pixel copy
//! of any Adobe asset.
//!
//! All palette colors are centralized as named [`egui::Color32`] constants so the
//! theme carries no scattered hex literals and the contrast/coherence tests can
//! reason about the exact values.

use eframe::egui::{self, FontFamily, FontId, Margin, Stroke, Style, Visuals};

// ----- Centralized palette (identical hex values to the previous inline literals) -----

/// Base panel background (module rail, side panels). Charcoal.
pub const PANEL: egui::Color32 = egui::Color32::from_rgb(0x2a, 0x2a, 0x2a);
/// Slightly lifted panel surface (faint backgrounds, non-interactive widgets).
pub const PANEL_LIGHT: egui::Color32 = egui::Color32::from_rgb(0x33, 0x33, 0x33);
/// Near-black working/navigator area.
pub const WORKING: egui::Color32 = egui::Color32::from_rgb(0x14, 0x14, 0x14);
/// Primary UI text.
pub const TEXT: egui::Color32 = egui::Color32::from_rgb(0xd8, 0xd8, 0xd8);
/// De-emphasized UI text (labels, hints).
pub const TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(0x9a, 0x9a, 0x9a);
/// Thin separators / window border.
pub const SEPARATOR: egui::Color32 = egui::Color32::from_rgb(0x40, 0x40, 0x40);
/// Single selection/activation accent (blue).
pub const ACCENT: egui::Color32 = egui::Color32::from_rgb(0x4a, 0x90, 0xd9);
/// Hovered widget fill (lifted above `PANEL_LIGHT`).
pub const HOVERED: egui::Color32 = egui::Color32::from_rgb(0x3c, 0x3c, 0x3c);

/// WCAG relative-luminance thresholds used by the contrast tests, with rationale.
///
/// * AAA normal text: `7.0` — highest legibility for primary reading text.
/// * AA normal text: `4.5` — minimum legibility for secondary / dim text.
/// * Non-text UI component: `3.0` — minimum contrast for graphical components
///   (accents, borders, fills) that are not body text.
///
/// Upper bound for any background/surface luminance in a dark theme: `0.12`.
/// sRGB mid-grey is roughly luminance `0.18`; a dark theme must stay clearly
/// below that. `0.12` leaves generous headroom beneath mid-grey while still
/// admitting the (already very dark) lifted panel / working surfaces.
///
/// Apply the Lumina dark theme to the given context.  Cheap enough to call once
/// per frame; `egui` only re-applies what changed.  The palette is built
/// exclusively from the named [`PANEL`], [`PANEL_LIGHT`], [`WORKING`], [`TEXT`],
/// [`TEXT_DIM`], [`SEPARATOR`], [`ACCENT`] and [`HOVERED`] constants above.
pub fn apply_lightroom_dark(ctx: &egui::Context) {
    let mut style = Style::default();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.window_margin = Margin::symmetric(8, 6);
    style.spacing.indent = 10.0;
    style.spacing.slider_width = 120.0;
    style.spacing.combo_width = 120.0;
    style.visuals.button_frame = false;

    let mut visuals = Visuals::dark();
    visuals.window_fill = PANEL;
    visuals.panel_fill = PANEL;
    visuals.faint_bg_color = PANEL_LIGHT;
    visuals.extreme_bg_color = WORKING;
    visuals.code_bg_color = PANEL_LIGHT;
    visuals.override_text_color = Some(TEXT);
    visuals.widgets.noninteractive.bg_fill = PANEL_LIGHT;
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_DIM);
    visuals.widgets.inactive.bg_fill = PANEL_LIGHT;
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.bg_fill = HOVERED;
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.bg_fill = ACCENT;
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, egui::Color32::WHITE);
    visuals.widgets.open.bg_fill = PANEL_LIGHT;
    visuals.widgets.open.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.selection.bg_fill = ACCENT;
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.window_stroke = Stroke::new(1.0_f32, SEPARATOR);
    visuals.indent_has_left_vline = true;
    style.visuals = visuals;

    // Compact, slightly condensed UI font for the dense panel look.
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            FontId::new(15.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Button,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            FontId::new(10.5, FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            FontId::new(11.0, FontFamily::Monospace),
        ),
    ]
    .into();

    ctx.set_style(style);
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    // Design limits enforced by the contrast/coherence tests below.
    /// AAA normal text contrast ratio (highest legibility for primary reading text).
    const CONTRAST_AAA_TEXT: f32 = 7.0;
    /// AA normal text contrast ratio (minimum legibility for secondary / dim text).
    const CONTRAST_AA_TEXT: f32 = 4.5;
    /// Non-text UI component contrast ratio (accents, borders, fills).
    const CONTRAST_UI_COMPONENT: f32 = 3.0;
    /// Upper bound for any background/surface luminance in a dark theme (well below
    /// sRGB mid-grey at ~0.18) so the theme stays clearly dark.
    const BG_MAX_LUMINANCE: f32 = 0.12;
    /// Minimum contrast between the accent and any surface fill so it stands out.
    const ACCENT_DISTINCTION_MIN: f32 = 2.0;

    /// Linearize a single sRGB channel for the WCAG relative-luminance formula.
    fn linearize_channel(c: u8) -> f32 {
        let s = c as f32 / 255.0;
        if s <= 0.03928 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    }

    /// WCAG 2.1 relative luminance of an [`egui::Color32`].
    fn relative_luminance(c: Color32) -> f32 {
        let r = linearize_channel(c.r());
        let g = linearize_channel(c.g());
        let b = linearize_channel(c.b());
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    /// WCAG contrast ratio between two colors (always `>= 1.0`).
    fn contrast_ratio(a: Color32, b: Color32) -> f32 {
        let la = relative_luminance(a);
        let lb = relative_luminance(b);
        let (lighter, darker) = if la >= lb { (la, lb) } else { (lb, la) };
        (lighter + 0.05) / (darker + 0.05)
    }

    /// Every surface/fill color used by the theme.
    fn surface_colors() -> [Color32; 4] {
        [PANEL, PANEL_LIGHT, WORKING, HOVERED]
    }

    #[test]
    fn contrast_text_vs_panel_meets_aaa() {
        let ratio = contrast_ratio(TEXT, PANEL);
        assert!(
            ratio >= CONTRAST_AAA_TEXT,
            "text/panel contrast {ratio:.2} < {CONTRAST_AAA_TEXT}"
        );
    }

    #[test]
    fn contrast_text_dim_vs_panel_meets_aa() {
        let ratio = contrast_ratio(TEXT_DIM, PANEL);
        assert!(
            ratio >= CONTRAST_AA_TEXT,
            "text_dim/panel contrast {ratio:.2} < {CONTRAST_AA_TEXT}"
        );
    }

    #[test]
    fn contrast_text_vs_working_meets_aaa() {
        let ratio = contrast_ratio(TEXT, WORKING);
        assert!(
            ratio >= CONTRAST_AAA_TEXT,
            "text/working contrast {ratio:.2} < {CONTRAST_AAA_TEXT}"
        );
    }

    #[test]
    fn contrast_accent_vs_panel_meets_ui_component() {
        let ratio = contrast_ratio(ACCENT, PANEL);
        assert!(
            ratio >= CONTRAST_UI_COMPONENT,
            "accent/panel contrast {ratio:.2} < {CONTRAST_UI_COMPONENT}"
        );
    }

    #[test]
    fn all_backgrounds_are_dark() {
        for c in surface_colors() {
            let lum = relative_luminance(c);
            assert!(
                lum < BG_MAX_LUMINANCE,
                "surface {c:?} luminance {lum:.4} >= {BG_MAX_LUMINANCE}"
            );
        }
    }

    #[test]
    fn no_two_surface_colors_are_identical() {
        let surfaces = surface_colors();
        for i in 0..surfaces.len() {
            for j in (i + 1)..surfaces.len() {
                assert_ne!(surfaces[i], surfaces[j], "surface {i} and {j} identical");
            }
        }
    }

    #[test]
    fn accent_stands_out_from_every_surface() {
        for c in surface_colors() {
            let ratio = contrast_ratio(ACCENT, c);
            assert!(
                ratio >= ACCENT_DISTINCTION_MIN,
                "accent/surface {c:?} contrast {ratio:.2} < {ACCENT_DISTINCTION_MIN}"
            );
        }
    }

    #[test]
    fn apply_lightroom_dark_sets_the_palette() {
        let ctx = egui::Context::default();
        apply_lightroom_dark(&ctx);
        let style = ctx.style();
        let visuals = &style.visuals;

        assert_eq!(visuals.window_fill, PANEL);
        assert_eq!(visuals.panel_fill, PANEL);
        assert_eq!(visuals.extreme_bg_color, WORKING);
        assert_eq!(visuals.faint_bg_color, PANEL_LIGHT);
        assert_eq!(visuals.code_bg_color, PANEL_LIGHT);
        assert_eq!(visuals.override_text_color, Some(TEXT));
        assert_eq!(visuals.widgets.noninteractive.bg_fill, PANEL_LIGHT);
        assert_eq!(
            visuals.widgets.noninteractive.fg_stroke,
            Stroke::new(1.0_f32, TEXT_DIM)
        );
        assert_eq!(visuals.widgets.inactive.bg_fill, PANEL_LIGHT);
        assert_eq!(
            visuals.widgets.inactive.fg_stroke,
            Stroke::new(1.0_f32, TEXT)
        );
        assert_eq!(visuals.widgets.hovered.bg_fill, HOVERED);
        assert_eq!(
            visuals.widgets.hovered.fg_stroke,
            Stroke::new(1.0_f32, TEXT)
        );
        assert_eq!(visuals.widgets.active.bg_fill, ACCENT);
        assert_eq!(
            visuals.widgets.active.fg_stroke,
            Stroke::new(1.0_f32, Color32::WHITE)
        );
        assert_eq!(visuals.widgets.open.bg_fill, PANEL_LIGHT);
        assert_eq!(visuals.widgets.open.fg_stroke, Stroke::new(1.0_f32, TEXT));
        assert_eq!(visuals.selection.bg_fill, ACCENT);
        assert_eq!(visuals.selection.stroke, Stroke::new(1.0_f32, ACCENT));
        assert_eq!(visuals.window_stroke, Stroke::new(1.0_f32, SEPARATOR));
        assert!(visuals.indent_has_left_vline);
    }
}
