//! Validation shared by the portable brush-prompt schema.

use super::{invalid, BrushMark, SidecarError};

pub(crate) fn validate_brush_marks(marks: &[BrushMark]) -> Result<(), SidecarError> {
    if marks.is_empty() {
        return invalid("prompt brush must contain at least one mark");
    }
    let in_unit = |value: f32| value.is_finite() && (0.0..=1.0).contains(&value);
    for mark in marks {
        if !in_unit(mark.x)
            || !in_unit(mark.y)
            || !in_unit(mark.radius)
            || mark.radius <= 0.0
            || !in_unit(mark.softness)
            || !in_unit(mark.flow)
        {
            return invalid(
                "prompt brush marks must have finite normalized coordinates within 0..=1, positive radius, and softness/flow within 0..=1",
            );
        }
    }
    Ok(())
}
