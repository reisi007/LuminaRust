//! F-100 Develop sections: section indices, the Basic-section recipe keys,
//! the section-name lookup and the G-11 open-state policy.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 12, file-size ratchet);
//! the G-11 section open state moved here in Slice 18. One responsibility:
//! the F-100 Develop-section model — how a section is indexed and named and
//! which sections are open. Pure value types and a name lookup plus the
//! display-only open-state policy; no GUI behaviour change.

use super::*;

/// Number of F-100 Develop sections in [`LuminaApp::DEVELOP_SECTIONS`] order
/// (Basic, Tone Curve, Color, Detail, Effects, Optics, Geometry, Masking):
/// the G-11 solo-mode scope.
pub const SECTION_COUNT: usize = 8;
pub const SECTION_BASIC: usize = 0;
pub const SECTION_TONE_CURVE: usize = 1;
pub const SECTION_COLOR: usize = 2;
pub const SECTION_DETAIL: usize = 3;
pub const SECTION_EFFECTS: usize = 4;
pub const SECTION_OPTICS: usize = 5;
pub const SECTION_GEOMETRY: usize = 6;
pub const SECTION_MASKING: usize = 7;

/// LRPAR-G01-BASIC: flat recipe keys owned by the Basic section (panel-
/// Previous/Reset scope). `vibrance`/`saturation` live in the Color section
/// and are only touched by Basic through the B&W treatment stash path.
pub(crate) const BASIC_TONE_KEYS: &[&str] = &[
    "wb_temperature",
    "wb_tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
];

/// F-100 label of a Develop section index (G-11, routed through [`Str`]).
/// `None` for out-of-range indices.
pub fn section_name(index: usize) -> Option<&'static str> {
    match index {
        SECTION_BASIC => Some(Str::Basic.t()),
        SECTION_TONE_CURVE => Some(Str::ToneCurve.t()),
        SECTION_COLOR => Some(Str::Color.t()),
        SECTION_DETAIL => Some(Str::Detail.t()),
        SECTION_EFFECTS => Some(Str::Effects.t()),
        SECTION_OPTICS => Some(Str::Optics.t()),
        SECTION_GEOMETRY => Some(Str::Geometry.t()),
        SECTION_MASKING => Some(Str::Masking.t()),
        _ => None,
    }
}

impl LuminaApp {
    /// Whether solo mode is armed (G-11, read-only accessor).
    pub fn solo_mode(&self) -> bool {
        self.solo_mode
    }

    /// Set solo mode (G-11). Display-only session state. Enabling with several
    /// open sections deterministically keeps the first (lowest index) and
    /// closes the rest.
    pub fn set_solo_mode(&mut self, enabled: bool) {
        instrument_gui_action!(self, GuiAction::SetSoloMode);
        if self.solo_mode == enabled {
            return;
        }
        self.solo_mode = enabled;
        if enabled {
            if let Some(first) = (0..SECTION_COUNT).find(|&i| self.section_open[i]) {
                for i in 0..SECTION_COUNT {
                    self.section_open[i] = i == first;
                }
            }
        }
        log::info!("GUI interaction: set_solo_mode -> {enabled}");
        self.status = if enabled {
            Str::SoloModeOn.t().into()
        } else {
            Str::SoloModeOff.t().into()
        };
    }

    /// Whether Develop section `index` is open (G-11, read-only accessor).
    /// Out-of-range indices read as closed.
    pub fn is_section_open(&self, index: usize) -> bool {
        self.section_open.get(index).copied().unwrap_or(false)
    }

    /// Set a Develop section open state (G-11). With solo mode on, opening one
    /// section closes the other seven. Out-of-range indices are refused loudly
    /// (warn, no state change). Display-only session state: never touches the
    /// recipe or the sidecar.
    pub fn set_section_open(&mut self, index: usize, open: bool) {
        if index >= SECTION_COUNT {
            log::warn!("GUI interaction: set_section_open refused for index {index}");
            return;
        }
        if open && self.solo_mode {
            for i in 0..SECTION_COUNT {
                self.section_open[i] = false;
            }
        }
        if self.section_open[index] != open {
            self.section_open[index] = open;
            log::info!("GUI interaction: set_section_open {index} -> {open}");
        }
    }
}
