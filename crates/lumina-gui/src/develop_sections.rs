//! F-100 Develop sections: section indices, the Basic-section recipe keys,
//! and the section-name lookup.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 12, file-size ratchet).
//! Pure value types and a name lookup; no GUI behaviour change.

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
