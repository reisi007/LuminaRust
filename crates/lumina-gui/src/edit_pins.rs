//! G-11 edit pins: kinds, pin records, and prompt-to-anchor derivation.
//!
//! Extracted from `lib.rs` (GUI-LIB-SPLIT-65 Slice 3, file-size ratchet).
//! Pure data types and a pure helper; no GUI behaviour change.

use super::*;

/// What an edit pin marks (G-11): a mask-library entry or a spot heal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditPinKind {
    Mask,
    Spot,
}

/// One headless-testable edit pin (G-11): normalized source-space anchor
/// (`0..=1`), selection flag and a stable id. The painter loop paints exactly
/// this list, so pin behaviour is covered without pixel assertions.
#[derive(Debug, Clone, PartialEq)]
pub struct EditPin {
    pub id: String,
    pub label: String,
    pub pos: (f32, f32),
    pub selected: bool,
    pub kind: EditPinKind,
}

/// Derive a pin anchor (normalized source space) from a mask prompt (G-11).
/// Box → rect centre; Brush → first mark; Polygon → first vertex; Ellipse →
/// centre; Gradient → midpoint of the start→end stretch along `angle_deg`
/// around the frame centre, clamped to `0..=1`. Prompts without geometry
/// (empty brush/polygon, G-03 range stages) yield `None`: no pin instead of
/// an invented position.
/// Pure function, unit-tested headless.
pub fn pin_anchor_for_prompt(prompt: &MaskPrompt) -> Option<(f32, f32)> {
    let clamp01 = |v: f32| v.clamp(0.0, 1.0);
    match prompt {
        MaskPrompt::Box { rect, .. } => {
            if !rect.x.is_finite()
                || !rect.y.is_finite()
                || !rect.width.is_finite()
                || !rect.height.is_finite()
            {
                return None;
            }
            Some((
                clamp01(rect.x + rect.width / 2.0),
                clamp01(rect.y + rect.height / 2.0),
            ))
        }
        MaskPrompt::Brush { marks, .. } => {
            let first = marks.first()?;
            if !first.x.is_finite() || !first.y.is_finite() {
                return None;
            }
            Some((clamp01(first.x), clamp01(first.y)))
        }
        MaskPrompt::Polygon { points, .. } => {
            let first = points.first()?;
            if !first.x.is_finite() || !first.y.is_finite() {
                return None;
            }
            Some((clamp01(first.x), clamp01(first.y)))
        }
        MaskPrompt::Ellipse { center, .. } => {
            if !center.x.is_finite() || !center.y.is_finite() {
                return None;
            }
            Some((clamp01(center.x), clamp01(center.y)))
        }
        MaskPrompt::Gradient {
            angle_deg,
            start,
            end,
            ..
        } => {
            if !angle_deg.is_finite() || !start.is_finite() || !end.is_finite() {
                return None;
            }
            let mid = (start + end) / 2.0;
            let radians = angle_deg.to_radians();
            Some((
                clamp01(0.5 + radians.cos() * (mid - 0.5)),
                clamp01(0.5 + radians.sin() * (mid - 0.5)),
            ))
        }
        // G-03 range stages are parameter boxes, not spatial prompts: no pin.
        MaskPrompt::ColorRange { .. } | MaskPrompt::LuminanceRange { .. } => None,
    }
}
