//! F-100 audit surfaces (R5-DUST-23-FOLLOWUP extraction from
//! `tests/f100_audit.rs`, keeping the audit table inside the file-size
//! ratchet).
//!
//! [`F100Surface`] is the headless draw surface hosting an action's clickable
//! button; [`ButtonRef`] locates the button (painted text, vector icon by
//! stable widget id, or interaction widget by id). Pure descriptor types —
//! the frame painters and the assertion stay in `f100_audit.rs`.

use super::*;
// UX-LOOK-TOOLBAR-18: icon buttons are audited by their stable widget id.
use crate::icon_toolbar::ToolbarIcon;

/// The headless draw surface that hosts an action's clickable button.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum F100Surface {
    Preview,
    Histogram,
    Rating,
    LibraryGrid,
    History,
    ModuleBar,
    Develop,
    Export,
    Basic,
    Masking,
    Spot,
    Merge,
    Geometry,
    Metadata,
    Detail,
    Optics,
    ToneCurve,
    Presets,
    // GUI-INSTRDBG-17c: Color (Point Color) and the generative canvas
    // buttons.
    Color,
    Generative,
    // GUI-INSTRDBG-17c-Rework F-1: the filmstrip selection buttons
    // (Sync Settings / Match Total Exposures / Previous Image).
    Filmstrip,
    // GUI-INSTRDBG-17c-Rest: the Library People view (per-face
    // "Use as mask" Develop bridge).
    People,
}

/// UX-LOOK-TOOLBAR-18: a clickable button is either a painted text label or a
/// vector-painted icon located by its stable widget id. `From` keeps the many
/// existing text entries readable as `.into()`.
///
/// UX-LOOK-TONECURVE-18: `Widget` covers graph-hosted gestures that have no
/// text label (the tone-curve graph adds on click, removes on double-click);
/// it is audited by the widget id it registers via `Ui::interact`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum ButtonRef {
    Text(String),
    Icon(ToolbarIcon),
    Widget(egui::Id),
}

impl From<String> for ButtonRef {
    fn from(value: String) -> Self {
        Self::Text(value)
    }
}

impl From<&str> for ButtonRef {
    fn from(value: &str) -> Self {
        Self::Text(value.to_string())
    }
}
