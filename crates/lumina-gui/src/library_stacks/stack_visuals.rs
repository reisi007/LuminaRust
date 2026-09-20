//! R5-STACKVIS-21 (User-Order 2026-09-20): the painted **stack membership
//! sign** of the Library Grid / Filmstrip.
//!
//! The user complained that stack membership reused the blue selection frame.
//! A stacked cell therefore gets its **own** sign: an amber/gold bracket plus
//! the stack optic of two offset cards behind the position badge. Selection
//! keeps the blue [`crate::theme::ACCENT`] stroke; both marks may be visible at
//! once and stay distinguishable by color **and** form.
//!
//! Extracted from `library_stacks.rs` (file-size ratchet `DoD.md` §8): the data
//! and mutation logic stays there, only the paint primitives live here.
//! Display-only — never recipe, never sidecar.

use super::*;

/// Amber/gold membership bracket (`#E8A91C`), deliberately far from the blue
/// selection accent (`theme::ACCENT` = `#4A90D9`). Pinned by
/// `tests::g15_stacks::r5_stacks::stack_sign_colour_and_form_differ_from_selection`.
pub(crate) const STACK_MEMBERSHIP_FRAME: egui::Color32 = egui::Color32::from_rgb(0xE8, 0xA9, 0x1C);

/// Darker amber (`#8A5E0E`) of the offset stack cards, so the two card outlines
/// behind the badge read as layers rather than one flat edge.
pub(crate) const STACK_MEMBERSHIP_CARD: egui::Color32 = egui::Color32::from_rgb(0x8A, 0x5E, 0x0E);

/// Paint the amber membership bracket around a stacked cell (`rect` is the
/// cell rectangle). Offset 1px outside so it never overdraws the thumbnail
/// edge, mirroring the selection stroke's `Outside` placement (which sits at
/// +2px) so both stay visible at once without covering each other.
pub(crate) fn paint_membership_frame(painter: &egui::Painter, rect: egui::Rect) {
    painter.rect_stroke(
        rect.expand(1.0),
        2.0,
        egui::Stroke::new(1.5_f32, STACK_MEMBERSHIP_FRAME),
        egui::StrokeKind::Outside,
    );
}

/// Paint the stack optic: two offset rounded card outlines behind the position
/// `badge`, peeking out to the bottom-right. Drawn **before** the badge fill so
/// only the offset card edges surface. Both use the stack card color, never the
/// selection accent.
pub(crate) fn paint_offset_cards(painter: &egui::Painter, badge: egui::Rect) {
    for offset in [egui::vec2(3.0, 3.0), egui::vec2(6.0, 6.0)] {
        painter.rect_stroke(
            badge.translate(offset),
            2.0,
            egui::Stroke::new(1.0_f32, STACK_MEMBERSHIP_CARD),
            egui::StrokeKind::Middle,
        );
    }
}

/// The Library surface a stack badge is painted on. R5-STACK-2: the grid and
/// the filmstrip are drawn in the **same** frame and both show the same image,
/// so a badge id derived from `thumb_key` alone collided — egui keys widget
/// interaction by id, so the second registration made one surface's badge
/// unclickable. The surface is part of the id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StackBadgeSurface {
    Grid,
    Filmstrip,
}

/// Stable egui id of the painted stack badge for one cell, so the headless
/// tests can locate and click it (F-100 clickability). Unique per surface
/// (R5-STACK-2).
pub(crate) fn stack_badge_id(surface: StackBadgeSurface, thumb_key: &str) -> egui::Id {
    egui::Id::new(("lumina-stack-badge", surface as u8, thumb_key))
}

impl LuminaApp {
    /// Paints the stack membership sign and the clickable position badge over a
    /// grid/filmstrip cell. Returns `true` when the badge was clicked, so the
    /// caller toggles the collapse instead of running the plain cell click. No
    /// sign is painted for a non-stacked entry (zero visual change for existing
    /// listings).
    pub(crate) fn paint_stack_badge(
        &self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        entry: &FileBrowserEntry,
        surface: StackBadgeSurface,
    ) -> bool {
        let Some(stack) = entry.stack.as_ref() else {
            return false;
        };
        let collapsed = self.stack_collapsed_for_entry(entry);
        let symbol = if collapsed { "⊞" } else { "⊟" };
        let count = stack.members.len();
        let index = stack
            .members
            .iter()
            .position(|member| member == &entry.name)
            .map(|position| position + 1)
            .unwrap_or(1);
        // R5-STACKVIS-21 (User-Entscheid 2026-09-20): the membership sign is
        // its own amber bracket + offset cards, never the blue selection frame.
        paint_membership_frame(ui.painter(), rect);
        let label = format!("{symbol} {index}/{count}");
        // Top-centre keeps clear of the folder badge (top-left), the
        // assisted-culling badge (top-right) and the rating/flag/color label
        // badge (bottom edge), so no two chips overlap.
        let badge = egui::Rect::from_min_size(
            egui::pos2(rect.center().x - 26.0, rect.top() + 2.0),
            egui::vec2(52.0, 16.0),
        );
        let response = ui.interact(
            badge,
            stack_badge_id(surface, &entry.thumb_key),
            egui::Sense::click(),
        );
        // Stack optic: two offset cards behind the badge (drawn before the
        // badge fill, so only their offset edges peek out).
        paint_offset_cards(ui.painter(), badge);
        ui.painter().rect_filled(
            badge,
            2.0,
            egui::Color32::from_rgba_unmultiplied(20, 20, 20, 180),
        );
        ui.painter().text(
            badge.left_center() + egui::vec2(4.0, 0.0),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            egui::Color32::WHITE,
        );
        response.clicked()
    }
}
