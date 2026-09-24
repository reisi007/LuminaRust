//! Deterministic CPU rasterization for persisted brush prompts.
//!
//! The coverage domain and min-source-axis geometry match
//! [`crate::mask_tiles::stamp_brush_mark_with_options`], so the full CPU matte
//! and incremental live/GPU plane stamps use one kernel.

use lumina_sidecar::BrushMark;

use crate::mask_tiles::{blend_brush_value, brush_mark_alpha};

/// Rasterize ordered brush marks into a preallocated row-major `u16` plane.
/// Positive marks select the largest coverage; negative marks select the
/// smallest complementary value, matching the normative sidecar contract.
pub(crate) fn rasterize_brush_marks_into(
    marks: &[BrushMark],
    width: u32,
    height: u32,
    values: &mut [u16],
) {
    debug_assert!(width > 0 && height > 0);
    debug_assert_eq!(values.len(), width as usize * height as usize);
    let width_f = width as f32;
    let height_f = height as f32;
    let min_axis = width.min(height) as f32;
    for (y, row) in values.chunks_exact_mut(width as usize).enumerate() {
        let source_y = (y as f32 + 0.5) / height_f;
        for (x, pixel) in row.iter_mut().enumerate() {
            let source_x = (x as f32 + 0.5) / width_f;
            let mut value = 0u16;
            for mark in marks {
                let dx_px = (source_x - mark.x) * width_f;
                let dy_px = (source_y - mark.y) * height_f;
                let distance = (dx_px * dx_px + dy_px * dy_px).sqrt() / min_axis;
                let alpha = brush_mark_alpha(distance, mark.radius, mark.softness, mark.flow);
                if alpha != 0 {
                    value = blend_brush_value(value, mark.sign, alpha);
                }
            }
            *pixel = value;
        }
    }
}
