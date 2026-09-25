//! Output-space alignment for local mask adjustments (`MASK-LOCAL-P0`).
//!
//! A mask is persisted in source-oriented coordinates.  The P0 compositor must
//! not blindly resize that plane to whatever frame happens to be current: a
//! zoom ROI, crop, quarter-turn or mirror is a coordinate transform.  This
//! module performs the explicit source → ROI → recipe-geometry → output
//! mapping and refuses transforms whose pixel-domain semantics are not
//! defined yet (lens/perspective/generative geometry is gated before it gets
//! here).

use crate::masks::MaskPlane;
use lumina_sidecar::{EditRecipe, Geometry, MaskDefinition};

/// Resample a u16 plane with the same pixel-centre convention as the existing
/// mask stage.  This is only used for a declared source-resolution or output
/// scale change; it is never used to compensate for an unknown geometry.
pub(crate) fn resample_plane_bilinear(
    plane: &MaskPlane,
    target_width: u32,
    target_height: u32,
) -> Result<MaskPlane, String> {
    if plane.width == 0 || plane.height == 0 || target_width == 0 || target_height == 0 {
        return Err("mask alignment cannot use a zero-dimension plane".into());
    }
    if plane.values.len() != (plane.width as usize) * (plane.height as usize) {
        return Err("mask alignment received a plane with an invalid value count".into());
    }
    let (sw, sh) = (plane.width as f32, plane.height as f32);
    let (tw, th) = (target_width as f32, target_height as f32);
    let mut values = Vec::with_capacity(target_width as usize * target_height as usize);
    for y in 0..target_height {
        let sy = ((y as f32 + 0.5) * sh / th - 0.5).clamp(0.0, sh - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(plane.height - 1);
        let fy = sy - y0 as f32;
        for x in 0..target_width {
            let sx = ((x as f32 + 0.5) * sw / tw - 0.5).clamp(0.0, sw - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(plane.width - 1);
            let fx = sx - x0 as f32;
            let at = |xx: u32, yy: u32| plane.values[(yy * plane.width + xx) as usize] as f32;
            let top = at(x0, y0) * (1.0 - fx) + at(x1, y0) * fx;
            let bottom = at(x0, y1) * (1.0 - fx) + at(x1, y1) * fx;
            let value = (top * (1.0 - fy) + bottom * fy).round();
            values.push(value.clamp(0.0, u16::MAX as f32) as u16);
        }
    }
    MaskPlane::new(target_width, target_height, values)
        .map_err(|error| format!("mask alignment resample failed: {error}"))
}

/// Align a resolved plane to the exact frame geometry consumed by a local
/// adjustment. `source_roi` is normalized `[x, y, width, height]` in the
/// mask definition's source-oriented coordinate system.
pub(crate) fn align_local_mask_plane(
    plane: &MaskPlane,
    definition: &MaskDefinition,
    input_width: u32,
    input_height: u32,
    recipe: &EditRecipe,
    source_roi: Option<[f32; 4]>,
) -> Result<MaskPlane, String> {
    let source_width = definition.geometry_context.width;
    let source_height = definition.geometry_context.height;
    if source_width == 0 || source_height == 0 {
        return Err(format!(
            "mask `{}/{}` has an empty source geometry context",
            definition.id, definition.id
        ));
    }

    // First put the artifact/prompt into the source coordinate system declared
    // by its identity.  This is an explicit, deterministic rescale of a matte,
    // not a guessed alignment for a transformed output.
    let mut aligned = resample_plane_bilinear(plane, source_width, source_height)?;
    if let Some(roi) = source_roi {
        validate_source_roi(roi)?;
        let crop = lumina_sidecar::Crop::Free {
            x: roi[0],
            y: roi[1],
            width: roi[2],
            height: roi[3],
        };
        let (x, y, width, height) = crate::crop_rect(source_width, source_height, Some(&crop))
            .map_err(|error| format!("mask ROI crop is invalid: {error}"))?;
        aligned = crop_plane(&aligned, x, y, width, height)?;
    }

    // `input_*` is the frame after any GUI zoom-ROI crop and before the recipe
    // geometry stage.  Resampling after the ROI crop keeps the same normalized
    // source mapping for full-frame, capped and zoomed previews.
    aligned = resample_plane_bilinear(&aligned, input_width, input_height)?;

    if let Some(geometry) = recipe.geometry.as_ref() {
        aligned = apply_geometry(&aligned, geometry)?;
    }
    // `input_*` describes the frame *before* the recipe geometry stage, so a
    // crop or quarter turn legitimately changes the size here. Only a
    // degenerate result is an error at this point; the caller additionally
    // checks the plane against the real output frame after geometry.
    if aligned.width == 0 || aligned.height == 0 {
        return Err("mask alignment produced a zero-dimension output".into());
    }
    Ok(aligned)
}

pub(crate) fn validate_source_roi(roi: [f32; 4]) -> Result<(), String> {
    let [x, y, width, height] = roi;
    if !roi.iter().all(|value| value.is_finite())
        || width <= 0.0
        || height <= 0.0
        || x < 0.0
        || y < 0.0
        || x > 1.0
        || y > 1.0
        || x + width > 1.0 + 1e-6
        || y + height > 1.0 + 1e-6
    {
        return Err(format!(
            "mask source ROI must be finite and normalized inside 0..=1, got {roi:?}"
        ));
    }
    Ok(())
}

fn crop_plane(
    plane: &MaskPlane,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
) -> Result<MaskPlane, String> {
    if width == 0 || height == 0 {
        return Err("mask crop produced a zero dimension".into());
    }
    let mut values = Vec::with_capacity(width as usize * height as usize);
    for row in 0..height {
        let start = ((y + row) * plane.width + x) as usize;
        let end = start + width as usize;
        values.extend_from_slice(&plane.values[start..end]);
    }
    MaskPlane::new(width, height, values).map_err(|error| format!("mask crop failed: {error}"))
}

fn apply_geometry(plane: &MaskPlane, geometry: &Geometry) -> Result<MaskPlane, String> {
    if geometry.version != 1 {
        return Err(format!(
            "local mask adjustments require geometry version 1, got {}",
            geometry.version
        ));
    }
    let mut result = if let Some(crop) = geometry.crop.as_ref() {
        let (x, y, width, height) = crate::crop_rect(plane.width, plane.height, Some(crop))
            .map_err(|error| format!("local mask crop is invalid: {error}"))?;
        crop_plane(plane, x, y, width, height)?
    } else {
        plane.clone()
    };
    if !is_quarter_turn(geometry.rotation_degrees) {
        return Err(format!(
            "local mask adjustments support only 90-degree rotation, got {} degrees",
            geometry.rotation_degrees
        ));
    }
    result = rotate_quarter_turn(&result, geometry.rotation_degrees)?;
    if geometry.mirror_horizontal {
        flip_horizontal(&mut result);
    }
    if geometry.mirror_vertical {
        flip_vertical(&mut result);
    }
    Ok(result)
}

fn is_quarter_turn(degrees: f32) -> bool {
    if !degrees.is_finite() {
        return false;
    }
    let turns = (degrees / 90.0).round();
    (degrees - turns * 90.0).abs() < 1e-4
}

fn rotate_quarter_turn(plane: &MaskPlane, degrees: f32) -> Result<MaskPlane, String> {
    let turns = (degrees / 90.0).round().rem_euclid(4.0) as i32;
    if turns == 0 {
        return Ok(plane.clone());
    }
    let (output_width, output_height) = if turns % 2 == 0 {
        (plane.width, plane.height)
    } else {
        (plane.height, plane.width)
    };
    let mut values = vec![0u16; output_width as usize * output_height as usize];
    for y in 0..plane.height {
        for x in 0..plane.width {
            let (dx, dy) = match turns {
                1 => (plane.height - 1 - y, x),
                2 => (plane.width - 1 - x, plane.height - 1 - y),
                3 => (y, plane.width - 1 - x),
                _ => unreachable!(),
            };
            values[(dy * output_width + dx) as usize] =
                plane.values[(y * plane.width + x) as usize];
        }
    }
    MaskPlane::new(output_width, output_height, values)
        .map_err(|error| format!("mask rotation failed: {error}"))
}

fn flip_horizontal(plane: &mut MaskPlane) {
    for y in 0..plane.height {
        for x in 0..plane.width / 2 {
            plane.values.swap(
                (y * plane.width + x) as usize,
                (y * plane.width + plane.width - 1 - x) as usize,
            );
        }
    }
}

fn flip_vertical(plane: &mut MaskPlane) {
    for y in 0..plane.height / 2 {
        for x in 0..plane.width {
            plane.values.swap(
                (y * plane.width + x) as usize,
                ((plane.height - 1 - y) * plane.width + x) as usize,
            );
        }
    }
}
