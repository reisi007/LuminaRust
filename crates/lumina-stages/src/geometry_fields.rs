//! G-06 geometry sub-stage accessors and `FIELD:VALUE` parsers, shared by the
//! CLI and the MCP server.
//!
//! Moved verbatim out of `crates/lumina-cli/src/main.rs` (`geometry_mut`,
//! `lens_mut`, `perspective_mut`, `parse_aspect_preset`, `parse_crop_free`,
//! `parse_mirror`, `parse_lens_field`, `set_lens_field`,
//! `parse_perspective_field`, `set_perspective_field`) so both transports
//! resolve, parse and apply geometry fields through ONE implementation — the
//! reason an unknown field name aborts with the same text on both sides.

use crate::copy::copy_mut;
use crate::error::StageError;
use lumina_sidecar::{AspectPreset, Geometry, LensCorrection, Perspective, SidecarDocument};

/// Mutable access to one virtual copy's geometry stage, creating an identity
/// stage when none exists (loud on unknown ids).
pub fn stage_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Geometry, StageError> {
    let copy = copy_mut(document, copy_id)?;
    Ok(copy.recipe.geometry.get_or_insert(Geometry {
        version: 1,
        crop: None,
        rotation_degrees: 0.0,
        mirror_horizontal: false,
        mirror_vertical: false,
    }))
}

/// Mutable access to one virtual copy's manual lens-correction stage, creating
/// an empty stage when none exists (loud on unknown ids).
pub fn lens_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut LensCorrection, StageError> {
    let copy = copy_mut(document, copy_id)?;
    Ok(copy.recipe.lens_correction.get_or_insert(LensCorrection {
        version: 1,
        profile: None,
        distortion_k1: None,
        distortion_k2: None,
        distortion_k3: None,
        vignette_c0: None,
        vignette_c1: None,
        vignette_c2: None,
        ca_red: None,
        ca_blue: None,
    }))
}

/// Mutable access to one virtual copy's manual perspective stage, creating an
/// identity stage when none exists (loud on unknown ids).
pub fn perspective_mut<'a>(
    document: &'a mut SidecarDocument,
    copy_id: &str,
) -> Result<&'a mut Perspective, StageError> {
    let copy = copy_mut(document, copy_id)?;
    Ok(copy.recipe.perspective.get_or_insert(Perspective {
        version: 1,
        vertical: 0.0,
        horizontal: 0.0,
        rotation: 0.0,
        scale: 1.0,
        aspect_ratio: 1.0,
        shift_x: 0.0,
        shift_y: 0.0,
    }))
}

/// Parses an aspect preset name (loud on unknown names — never a guess).
pub fn parse_aspect_preset(value: &str) -> Result<AspectPreset, StageError> {
    match value {
        "original" => Ok(AspectPreset::Original),
        "1:1" => Ok(AspectPreset::OneToOne),
        "4:5" => Ok(AspectPreset::FourToFive),
        "5:4" => Ok(AspectPreset::FiveToFour),
        "3:2" => Ok(AspectPreset::ThreeToTwo),
        "2:3" => Ok(AspectPreset::TwoToThree),
        "4:3" => Ok(AspectPreset::FourToThree),
        "3:4" => Ok(AspectPreset::ThreeToFour),
        "16:9" => Ok(AspectPreset::SixteenToNine),
        "9:16" => Ok(AspectPreset::NineToSixteen),
        _ => Err(StageError::Message(format!(
            "invalid aspect preset `{value}`: expected one of original|1:1|4:5|5:4|3:2|2:3|4:3|3:4|16:9|9:16"
        ))),
    }
}

/// Parses a free crop rectangle as `x,y,w,h` (loud on malformed input; range
/// geometry is validated on save, not guessed here).
pub fn parse_crop_free(value: &str) -> Result<(f32, f32, f32, f32), StageError> {
    let parts: Vec<&str> = value.split(',').collect();
    let numbers: Option<Vec<f32>> = parts
        .iter()
        .map(|part| part.trim().parse::<f32>().ok())
        .collect();
    match numbers.as_deref() {
        Some([x, y, width, height]) => Ok((*x, *y, *width, *height)),
        _ => Err(StageError::Message(format!(
            "invalid crop rect `{value}`: expected `x,y,w,h` with finite numbers"
        ))),
    }
}

/// Parses mirror flags as `h|v|hv|none` (loud on unknown words).
pub fn parse_mirror(value: &str) -> Result<(bool, bool), StageError> {
    match value {
        "h" => Ok((true, false)),
        "v" => Ok((false, true)),
        "hv" => Ok((true, true)),
        "none" => Ok((false, false)),
        _ => Err(StageError::Message(format!(
            "invalid mirror `{value}`: expected h|v|hv|none"
        ))),
    }
}

/// Parses one manual lens field as `FIELD:VALUE` (loud on unknown fields or
/// non-numbers; ranges are validated on save).
pub fn parse_lens_field(spec: &str) -> Result<(String, f32), StageError> {
    const FIELDS: &[&str] = &[
        "distortion_k1",
        "distortion_k2",
        "distortion_k3",
        "vignette_c0",
        "vignette_c1",
        "vignette_c2",
        "ca_red",
        "ca_blue",
    ];
    let (field, value) = spec.split_once(':').ok_or_else(|| {
        StageError::Message(format!(
            "invalid lens field `{spec}`: expected `FIELD:VALUE`"
        ))
    })?;
    if !FIELDS.contains(&field) {
        return Err(StageError::Message(format!(
            "invalid lens field `{field}`: expected one of {}",
            FIELDS.join("|")
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        StageError::Message(format!("invalid lens value in `{spec}`: not a number"))
    })?;
    Ok((field.into(), value))
}

/// Applies one parsed manual lens field (fields are pre-validated by
/// [`parse_lens_field`]).
pub fn set_lens_field(lens: &mut LensCorrection, field: &str, value: f32) {
    match field {
        "distortion_k1" => lens.distortion_k1 = Some(value),
        "distortion_k2" => lens.distortion_k2 = Some(value),
        "distortion_k3" => lens.distortion_k3 = Some(value),
        "vignette_c0" => lens.vignette_c0 = Some(value),
        "vignette_c1" => lens.vignette_c1 = Some(value),
        "vignette_c2" => lens.vignette_c2 = Some(value),
        "ca_red" => lens.ca_red = Some(value),
        "ca_blue" => lens.ca_blue = Some(value),
        _ => unreachable!("lens field pre-validated by parse_lens_field"),
    }
}

/// Parses one manual perspective field as `FIELD:VALUE` (loud on unknown fields
/// or non-numbers; ranges are validated on save).
pub fn parse_perspective_field(spec: &str) -> Result<(String, f32), StageError> {
    const FIELDS: &[&str] = &[
        "vertical",
        "horizontal",
        "rotation",
        "scale",
        "aspect_ratio",
        "shift_x",
        "shift_y",
    ];
    let (field, value) = spec.split_once(':').ok_or_else(|| {
        StageError::Message(format!(
            "invalid perspective field `{spec}`: expected `FIELD:VALUE`"
        ))
    })?;
    if !FIELDS.contains(&field) {
        return Err(StageError::Message(format!(
            "invalid perspective field `{field}`: expected one of {}",
            FIELDS.join("|")
        )));
    }
    let value: f32 = value.trim().parse().map_err(|_| {
        StageError::Message(format!(
            "invalid perspective value in `{spec}`: not a number"
        ))
    })?;
    Ok((field.into(), value))
}

/// Applies one parsed manual perspective field (fields are pre-validated by
/// [`parse_perspective_field`]).
pub fn set_perspective_field(perspective: &mut Perspective, field: &str, value: f32) {
    match field {
        "vertical" => perspective.vertical = value,
        "horizontal" => perspective.horizontal = value,
        "rotation" => perspective.rotation = value,
        "scale" => perspective.scale = value,
        "aspect_ratio" => perspective.aspect_ratio = value,
        "shift_x" => perspective.shift_x = value,
        "shift_y" => perspective.shift_y = value,
        _ => unreachable!("perspective field pre-validated by parse_perspective_field"),
    }
}
