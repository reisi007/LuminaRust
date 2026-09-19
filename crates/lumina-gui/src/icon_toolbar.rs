//! UX-LOOK-TOOLBAR-18 (UXG-04): the icon tool strip and the iconified Library
//! view tabs.
//!
//! Every icon is painted from egui primitives (lines/circles/rects), never a
//! font glyph or emoji: the rendering is deterministic in the headless and
//! kittest-golden passes and carries no font-coverage dependency (`theme.rs`
//! centralizes the palette, `theme`/`ACCENT` drives the active highlight).
//!
//! The tooltips carry the *existing* working labels (`Str::*`) and the
//! *existing* shortcut hints — UX-LOOK-18's Namen-Vorbehalt forbids new final
//! wordings, and `i18n.rs` stays frozen under the file-size ratchet. The
//! buttons change no recipe/sidecar semantics: each one routes through the same
//! `toggle_*`/`set_*` path as its keyboard shortcut ([`ToolbarIcon::id`] is the
//! stable widget id headless tests click via `Context::read_response`).

use super::*;
use crate::theme;

/// The Dust-Removal panel already carries this exact working literal (no `Str`
/// variant exists; UXG-17's `Str`-Pflicht and the UX-LOOK-18 Namen-Vorbehalt
/// collide here). Reusing it verbatim keeps the toolbar consistent with the
/// panel until the Naming pass adds a real `Str`.
const HEAL_WORKING_LABEL: &str = "Heal (Q)";

/// One icon in the preview tool strip or the Library view-tab row. The enum is
/// the single source of truth for the stable widget id, the tooltip and the
/// painter, so a button can never paint without an accessible name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ToolbarIcon {
    // LR develop tool strip (Crop / Heal / Red-Eye / Masking order).
    Crop,
    Heal,
    RedEye,
    Masking,
    // Display-only view toggles (recipe-free, same path as `R`/`J`/`L`/`Tab`/`F`).
    Clipping,
    Split,
    LightsOut,
    Panels,
    AllPanels,
    Fullscreen,
    // Library view tabs (G / E / C / N / People).
    ViewGrid,
    ViewLoupe,
    ViewCompare,
    ViewSurvey,
    ViewPeople,
}

/// Every icon, in one place so the distinctness/paint tests stay exhaustive
/// (test-only: production always uses the explicit per-surface lists).
#[cfg(test)]
pub(crate) const ALL_TOOLBAR_ICONS: [ToolbarIcon; 15] = [
    ToolbarIcon::Crop,
    ToolbarIcon::Heal,
    ToolbarIcon::RedEye,
    ToolbarIcon::Masking,
    ToolbarIcon::Clipping,
    ToolbarIcon::Split,
    ToolbarIcon::LightsOut,
    ToolbarIcon::Panels,
    ToolbarIcon::AllPanels,
    ToolbarIcon::Fullscreen,
    ToolbarIcon::ViewGrid,
    ToolbarIcon::ViewLoupe,
    ToolbarIcon::ViewCompare,
    ToolbarIcon::ViewSurvey,
    ToolbarIcon::ViewPeople,
];

impl ToolbarIcon {
    /// Stable widget id. Global (not `Ui`-relative) so the headless tests can
    /// click exactly this button with `Context::read_response`, without a
    /// painted text label to search for.
    pub(crate) fn id(self) -> egui::Id {
        egui::Id::new(("lumina.toolbar_icon", self.key()))
    }

    /// Stable string key (debug output, id derivation, uniqueness tests).
    pub(crate) fn key(self) -> &'static str {
        match self {
            Self::Crop => "view.crop",
            Self::Heal => "view.heal",
            Self::RedEye => "view.red_eye",
            Self::Masking => "view.masking",
            Self::Clipping => "view.clipping",
            Self::Split => "view.split",
            Self::LightsOut => "view.lights_out",
            Self::Panels => "view.panels",
            Self::AllPanels => "view.all_panels",
            Self::Fullscreen => "view.fullscreen",
            Self::ViewGrid => "library.grid",
            Self::ViewLoupe => "library.loupe",
            Self::ViewCompare => "library.compare",
            Self::ViewSurvey => "library.survey",
            Self::ViewPeople => "library.people",
        }
    }

    /// Tooltip: existing `Str` working label (plus the existing shortcut where
    /// the label does not already carry it). No new final wording is coined.
    pub(crate) fn tooltip(self) -> String {
        match self {
            Self::Crop => Str::ViewToolbarCrop.t().to_string(),
            Self::Heal => HEAL_WORKING_LABEL.to_string(),
            Self::RedEye => Str::RedEye.t().to_string(),
            Self::Masking => format!("{} (K)", Str::Masking.t()),
            Self::Clipping => Str::ViewToolbarClipping.t().to_string(),
            Self::Split => Str::ViewToolbarSplit.t().to_string(),
            Self::LightsOut => Str::ViewToolbarLightsOut.t().to_string(),
            Self::Panels => Str::ViewToolbarPanels.t().to_string(),
            Self::AllPanels => Str::ViewToolbarAllPanels.t().to_string(),
            Self::Fullscreen => Str::ViewToolbarFullscreen.t().to_string(),
            Self::ViewGrid => Str::LibraryGridOn.t().to_string(),
            Self::ViewLoupe => Str::LoupeOn.t().to_string(),
            Self::ViewCompare => format!("{} (C)", Str::CompareModeCompare.t()),
            Self::ViewSurvey => Str::SurveyOn.t().to_string(),
            Self::ViewPeople => Str::FacePeople.t().to_string(),
        }
    }

    /// Paint the icon into `rect` (already shrunk to the icon box) in `color`.
    fn paint(self, painter: &egui::Painter, rect: egui::Rect, color: egui::Color32) {
        let stroke = egui::Stroke::new(icon_stroke_width(rect), color);
        match self {
            Self::Crop => paint_crop(painter, rect, stroke),
            Self::Heal => paint_heal(painter, rect, color, stroke),
            Self::RedEye => paint_red_eye(painter, rect, color, stroke),
            Self::Masking => paint_masking(painter, rect, color, stroke),
            Self::Clipping => paint_clipping(painter, rect, color, stroke),
            Self::Split => paint_split(painter, rect, color, stroke),
            Self::LightsOut => paint_lights_out(painter, rect, color, stroke),
            Self::Panels => paint_panels(painter, rect, color, stroke),
            Self::AllPanels => paint_all_panels(painter, rect, color, stroke),
            Self::Fullscreen => paint_fullscreen(painter, rect, stroke),
            Self::ViewGrid => paint_view_grid(painter, rect, stroke),
            Self::ViewLoupe => paint_view_loupe(painter, rect, color, stroke),
            Self::ViewCompare => paint_view_compare(painter, rect, color, stroke),
            Self::ViewSurvey => paint_view_survey(painter, rect, stroke),
            Self::ViewPeople => paint_view_people(painter, rect, stroke),
        }
    }
}

/// The Library view-tab icon for one [`LibraryView`] (exhaustive, no `_`).
pub(crate) fn library_view_icon(view: LibraryView) -> ToolbarIcon {
    match view {
        LibraryView::Grid => ToolbarIcon::ViewGrid,
        LibraryView::Loupe => ToolbarIcon::ViewLoupe,
        LibraryView::Compare => ToolbarIcon::ViewCompare,
        LibraryView::Survey => ToolbarIcon::ViewSurvey,
        LibraryView::People => ToolbarIcon::ViewPeople,
    }
}

/// Paint one icon button in the Lightroom tool strip / view-tab row. Hover and
/// active states use the centralized theme palette; the returned [`Response`]
/// carries the accessible label and the tooltip.
pub(crate) fn icon_button(ui: &mut egui::Ui, icon: ToolbarIcon, active: bool) -> egui::Response {
    // `allocate_space` reserves the layout slot without registering a second
    // (auto-id) interactive widget; `interact` then owns the stable icon id.
    let (_, rect) = ui.allocate_space(egui::vec2(26.0, 24.0));
    let response = ui.interact(rect, icon.id(), egui::Sense::click());
    if active {
        ui.painter().rect_filled(rect, 4.0, theme::ACCENT);
    } else if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, theme::HOVERED);
    }
    let color = if active {
        egui::Color32::WHITE
    } else {
        theme::TEXT
    };
    icon.paint(ui.painter(), rect.shrink(4.0), color);
    let tooltip = icon.tooltip();
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, icon.tooltip()));
    response.on_hover_text(tooltip)
}

// ---------------------------------------------------------------------------
// Primitive helpers.
// ---------------------------------------------------------------------------

/// Stroke width scaled to the icon box so every icon keeps a consistent weight.
fn icon_stroke_width(rect: egui::Rect) -> f32 {
    (rect.width().min(rect.height()) * 0.09).clamp(1.2, 1.9)
}

/// Normalized point inside `rect` (`0..1` on both axes).
fn p(rect: egui::Rect, x: f32, y: f32) -> egui::Pos2 {
    egui::pos2(
        rect.left() + rect.width() * x,
        rect.top() + rect.height() * y,
    )
}

/// A filled rectangle from normalized coordinates.
fn nrect(rect: egui::Rect, x0: f32, y0: f32, x1: f32, y1: f32) -> egui::Rect {
    egui::Rect::from_min_max(p(rect, x0, y0), p(rect, x1, y1))
}

/// Paint a polyline through normalized coordinates.
fn poly(painter: &egui::Painter, rect: egui::Rect, pts: &[(f32, f32)], stroke: egui::Stroke) {
    painter.add(egui::Shape::line(
        pts.iter().map(|&(x, y)| p(rect, x, y)).collect(),
        stroke,
    ));
}

// ---------------------------------------------------------------------------
// Develop tool strip icons.
// ---------------------------------------------------------------------------

/// Classic crop marks: two overlapping L-shaped guide lines.
fn paint_crop(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    poly(
        painter,
        rect,
        &[(0.30, 0.05), (0.30, 0.70), (0.95, 0.70)],
        stroke,
    );
    poly(
        painter,
        rect,
        &[(0.05, 0.30), (0.70, 0.30), (0.70, 0.95)],
        stroke,
    );
}

/// Spot-heal: spot circle outline with the removal dot inside.
fn paint_heal(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.circle_stroke(p(rect, 0.5, 0.5), rect.width() * 0.36, stroke);
    painter.circle_filled(p(rect, 0.5, 0.5), rect.width() * 0.13, color);
}

/// Red-eye: an eye outline with a filled pupil.
fn paint_red_eye(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    let (cx, cy, rx, ry) = (0.5, 0.5, 0.46, 0.28);
    let n = 12;
    let mut pts: Vec<egui::Pos2> = Vec::with_capacity(2 * (n + 1));
    for i in 0..=n {
        let t = (i as f32 / n as f32) * 2.0 - 1.0;
        pts.push(p(rect, cx + rx * t, cy - ry * (1.0 - t * t)));
    }
    for i in (0..=n).rev() {
        let t = (i as f32 / n as f32) * 2.0 - 1.0;
        pts.push(p(rect, cx + rx * t, cy + ry * (1.0 - t * t)));
    }
    painter.add(egui::Shape::line(pts, stroke));
    painter.circle_filled(p(rect, cx, cy), rect.width() * 0.14, color);
}

/// Masking: a brush with a filled tip.
fn paint_masking(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    poly(painter, rect, &[(0.86, 0.12), (0.47, 0.51)], stroke);
    painter.add(egui::Shape::convex_polygon(
        vec![
            p(rect, 0.44, 0.46),
            p(rect, 0.12, 0.88),
            p(rect, 0.52, 0.82),
        ],
        color,
        egui::Stroke::NONE,
    ));
}

// ---------------------------------------------------------------------------
// Display-only view toggles.
// ---------------------------------------------------------------------------

/// Clipping warnings: a frame with shadow/highlight triangles.
fn paint_clipping(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            p(rect, 0.08, 0.08),
            p(rect, 0.08, 0.52),
            p(rect, 0.52, 0.08),
        ],
        color,
        egui::Stroke::NONE,
    ));
    painter.add(egui::Shape::convex_polygon(
        vec![
            p(rect, 0.92, 0.92),
            p(rect, 0.92, 0.48),
            p(rect, 0.48, 0.92),
        ],
        color,
        egui::Stroke::NONE,
    ));
    painter.rect_stroke(
        nrect(rect, 0.08, 0.08, 0.92, 0.92),
        1.5,
        stroke,
        egui::StrokeKind::Inside,
    );
}

/// Before/After split: frame with a shaded left half and a center divider.
fn paint_split(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.rect_filled(
        nrect(rect, 0.1, 0.14, 0.5, 0.86),
        1.0,
        color.gamma_multiply(0.35),
    );
    painter.line_segment([p(rect, 0.5, 0.1), p(rect, 0.5, 0.9)], stroke);
    painter.rect_stroke(
        nrect(rect, 0.1, 0.1, 0.9, 0.9),
        1.5,
        stroke,
        egui::StrokeKind::Inside,
    );
}

/// Lights-out: a dimmed frame.
fn paint_lights_out(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.rect_stroke(
        nrect(rect, 0.1, 0.14, 0.9, 0.86),
        1.5,
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(
        nrect(rect, 0.28, 0.36, 0.72, 0.64),
        1.0,
        color.gamma_multiply(0.35),
    );
}

/// Side panels: frame with filled left/right rails.
fn paint_panels(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.rect_stroke(
        nrect(rect, 0.08, 0.14, 0.92, 0.86),
        1.5,
        stroke,
        egui::StrokeKind::Inside,
    );
    painter.rect_filled(nrect(rect, 0.08, 0.14, 0.26, 0.86), 0.5, color);
    painter.rect_filled(nrect(rect, 0.74, 0.14, 0.92, 0.86), 0.5, color);
}

/// All panels: frame with all four rails filled.
fn paint_all_panels(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    let outer = nrect(rect, 0.08, 0.12, 0.92, 0.88);
    painter.rect_stroke(outer, 1.5, stroke, egui::StrokeKind::Inside);
    let bar = 0.17;
    painter.rect_filled(nrect(rect, 0.08, 0.12, 0.92, 0.12 + bar), 0.5, color);
    painter.rect_filled(nrect(rect, 0.08, 0.88 - bar, 0.92, 0.88), 0.5, color);
    painter.rect_filled(nrect(rect, 0.08, 0.12, 0.08 + bar, 0.88), 0.5, color);
    painter.rect_filled(nrect(rect, 0.92 - bar, 0.12, 0.92, 0.88), 0.5, color);
}

/// Fullscreen: four corner brackets.
fn paint_fullscreen(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    let (a, b) = (0.1, 0.34);
    poly(painter, rect, &[(a, b), (a, a), (b, a)], stroke);
    poly(
        painter,
        rect,
        &[(1.0 - b, a), (1.0 - a, a), (1.0 - a, b)],
        stroke,
    );
    poly(
        painter,
        rect,
        &[(a, 1.0 - b), (a, 1.0 - a), (b, 1.0 - a)],
        stroke,
    );
    poly(
        painter,
        rect,
        &[(1.0 - b, 1.0 - a), (1.0 - a, 1.0 - a), (1.0 - a, 1.0 - b)],
        stroke,
    );
}

// ---------------------------------------------------------------------------
// Library view-tab icons.
// ---------------------------------------------------------------------------

/// Grid: four outlined tiles.
fn paint_view_grid(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    for (x0, y0) in [(0.1, 0.1), (0.54, 0.1), (0.1, 0.54), (0.54, 0.54)] {
        painter.rect_stroke(
            nrect(rect, x0, y0, x0 + 0.36, y0 + 0.36),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
}

/// Loupe: magnifier with a dot in the lens.
fn paint_view_loupe(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    painter.circle_stroke(p(rect, 0.42, 0.42), rect.width() * 0.30, stroke);
    painter.line_segment([p(rect, 0.64, 0.64), p(rect, 0.9, 0.9)], stroke);
    painter.circle_filled(p(rect, 0.42, 0.42), rect.width() * 0.1, color);
}

/// Compare: two frames side by side, left one shaded.
fn paint_view_compare(
    painter: &egui::Painter,
    rect: egui::Rect,
    color: egui::Color32,
    stroke: egui::Stroke,
) {
    let left = nrect(rect, 0.08, 0.14, 0.47, 0.86);
    painter.rect_filled(left, 1.0, color.gamma_multiply(0.35));
    painter.rect_stroke(left, 1.0, stroke, egui::StrokeKind::Inside);
    painter.rect_stroke(
        nrect(rect, 0.53, 0.14, 0.92, 0.86),
        1.0,
        stroke,
        egui::StrokeKind::Inside,
    );
}

/// Survey: three outlined frames in a row.
fn paint_view_survey(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    for x0 in [0.06, 0.37, 0.68] {
        painter.rect_stroke(
            nrect(rect, x0, 0.18, x0 + 0.26, 0.82),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    }
}

/// People: a head circle over a shoulder arc.
fn paint_view_people(painter: &egui::Painter, rect: egui::Rect, stroke: egui::Stroke) {
    painter.circle_stroke(p(rect, 0.5, 0.3), rect.width() * 0.17, stroke);
    let (rx, ry, cy) = (0.34, 0.24, 0.9);
    let mut pts: Vec<egui::Pos2> = Vec::with_capacity(13);
    for i in 0..=12 {
        let t = i as f32 / 12.0;
        let x = 0.5 - rx + 2.0 * rx * t;
        let y = cy - ry * (1.0 - (2.0 * t - 1.0).powi(2));
        pts.push(p(rect, x, y));
    }
    painter.add(egui::Shape::line(pts, stroke));
}
