//! Library folder/badge path helpers.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 5, file-size ratchet).
//! Pure lexical path logic and badge constants; no GUI behaviour change.

use super::*;

/// Root of the Library folder tree — the **workdir** itself (the current
/// `directory` field), per Lightroom-parity: the Folders panel shows the
/// working directory as the root, not the whole `$HOME` tree. Pure path logic
/// so headless tests can exercise it without mutating process environment
/// state. Previously this rooted at `$HOME` (or a grandparent); the user asked
/// for root = workdir so the Library only ever browses the opened folder.
pub(crate) fn library_root(directory: &str) -> PathBuf {
    let dir = Path::new(directory).to_path_buf();
    if dir.as_os_str().is_empty() {
        PathBuf::from(".")
    } else {
        dir
    }
}

/// Short display label of a folder node: path relative to the tree root, or
/// the final component for the root itself.
pub(crate) fn folder_label(root: &Path, path: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string()),
    }
}

/// Grid path badge of an entry vs. the listed `root`: `""` for top-level
/// files, otherwise the parent directory relative to `root` (F-100 Library:
/// recursive aggregation shows subfolder images with their relative folder
/// as badge). Pure lexical path logic, no I/O.
pub(crate) fn folder_badge(root: &Path, path: &Path) -> String {
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    match parent.strip_prefix(root) {
        Ok(relative) if !relative.as_os_str().is_empty() => relative.display().to_string(),
        _ => String::new(),
    }
}

/// Maximum display width of the Library grid path badge, in characters.
/// The badge box is fixed at 118px (`vec2(118.0, 16.0)`) with monospace
/// 11.0; a monospace glyph advances ~6.6px, so 17 chars (~112px) fit with
/// padding to spare. Longer badges are middle-truncated (see below); the
/// stored [`FileBrowserEntry::folder`] keeps the full path.
pub(crate) const FOLDER_BADGE_MAX_CHARS: usize = 17;

/// Library grid badge chip background (GUI-LIBRARY-BADGE-CONTRAST-1): a
/// solid mid-grey instead of translucent black. Over dark thumbnails the old
/// chip melted into the image ("dark on dark") while the white 11px monospace
/// text needs AA contrast — pinned by `library_badge_contrast_meets_aa`.
/// Shared by the path badge and the rating badge (same chip style).
/// UX-SLICE-2 (F4): public so the rated-badge pixel golden asserts against the
/// exact painted fill instead of a duplicated literal.
pub const LIBRARY_BADGE_BG: egui::Color32 = egui::Color32::from_rgb(0x42, 0x42, 0x42);

/// UX-SLICE-1 (UXG-07): color-coded render-state badges at the preview edge
/// (the hash text lives in the app status line, drawn in `LuminaApp`'s `ui`).
/// Amber = an in-flight low-res draft; red-orange = stale/pending (no
/// `render_key` yet).
pub(crate) const RENDER_STATE_DRAFT_COLOR: egui::Color32 =
    egui::Color32::from_rgb(0xE6, 0xB4, 0x32);
pub(crate) const RENDER_STATE_STALE_COLOR: egui::Color32 =
    egui::Color32::from_rgb(0xE0, 0x6C, 0x3C);

/// UX-SLICE-1 (P5): deterministic Library empty-state icon (a framed picture
/// with a sun and a mountain). Painted with primitives instead of an emoji so
/// no system font coverage can shift the golden.
pub(crate) fn paint_library_empty_icon(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(72.0, 54.0), egui::Sense::hover());
    let stroke = ui.visuals().weak_text_color();
    let line = egui::Stroke::new(1.5, stroke);
    ui.painter()
        .rect_stroke(rect, 4.0, line, egui::StrokeKind::Inside);
    ui.painter()
        .circle_filled(rect.left_top() + egui::vec2(18.0, 16.0), 4.0, stroke);
    let mountain = vec![
        rect.left_bottom() + egui::vec2(6.0, -6.0),
        rect.center_bottom() + egui::vec2(2.0, -24.0),
        rect.right_bottom() + egui::vec2(-6.0, -6.0),
    ];
    ui.painter().add(egui::Shape::convex_polygon(
        mountain,
        stroke,
        egui::Stroke::NONE,
    ));
}

/// UX-SLICE-1 (UXG-09): the LR-01 rating/flag/color-label badge text for an
/// entry, or `None` when the cell stays clean (unrated, unflagged and
/// unlabeled). Pure and unit-testable; shared by the Library grid and the
/// filmstrip so both views present the same `FileBrowserEntry` data.
pub(crate) fn entry_badge_text(entry: &FileBrowserEntry) -> Option<String> {
    if entry.rating == 0 && entry.flag == lumina_sidecar::Flag::Unflagged && entry.color_label == 0
    {
        return None;
    }
    let mut badge = match entry.flag {
        lumina_sidecar::Flag::Pick => format!("{} P", stars_for_rating(entry.rating)),
        lumina_sidecar::Flag::Reject => format!("{} X", stars_for_rating(entry.rating)),
        lumina_sidecar::Flag::Unflagged => stars_for_rating(entry.rating),
    };
    if entry.color_label > 0 {
        badge.push_str(&format!(" ●{}", color_label_name(entry.color_label)));
    }
    Some(badge)
}

/// UX-SLICE-1 (UXG-09): paint [`entry_badge_text`] over the bottom-left edge
/// of `rect` (Library grid + filmstrip share this presentation).
pub(crate) fn paint_entry_badge(ui: &egui::Ui, rect: egui::Rect, entry: &FileBrowserEntry) {
    let Some(badge) = entry_badge_text(entry) else {
        return;
    };
    let badge_pos = rect.left_bottom() + egui::vec2(4.0, -16.0);
    ui.painter().rect_filled(
        egui::Rect::from_min_size(badge_pos - egui::vec2(2.0, 2.0), egui::vec2(118.0, 16.0)),
        2.0,
        LIBRARY_BADGE_BG,
    );
    ui.painter().text(
        badge_pos,
        egui::Align2::LEFT_TOP,
        badge,
        egui::FontId::monospace(11.0),
        egui::Color32::WHITE,
    );
}

/// Display text for the folder badge: the full badge when it fits, otherwise
/// middle-truncated with `…` (`head…tail`) so the painted text never
/// overflows the fixed 118px box. The full name stays available via hover.
/// Pure string logic (char-based, unicode-safe), no I/O.
pub(crate) fn folder_badge_display(badge: &str) -> String {
    let len = badge.chars().count();
    if len <= FOLDER_BADGE_MAX_CHARS {
        return badge.to_owned();
    }
    let tail_len = (FOLDER_BADGE_MAX_CHARS - 1) / 2;
    let head_len = FOLDER_BADGE_MAX_CHARS - 1 - tail_len;
    let head: String = badge.chars().take(head_len).collect();
    let tail: String = badge.chars().skip(len - tail_len).collect();
    format!("{head}…{tail}")
}
