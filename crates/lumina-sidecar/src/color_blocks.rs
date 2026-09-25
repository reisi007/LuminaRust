//! The one set of range validators for the per-pixel color blocks.
//!
//! The global recipe (`EditRecipe::hsl`/`point_color`/`color_grading`) and the
//! typed mask-local recipe (MASK-LOCAL-P1.2b `local_adjustments.hsl` /
//! `.point_color` / `.color_grading`) store the *same* blocks and must accept
//! exactly the same values. Keeping the rules in one place is what makes that
//! a structural property instead of two validators that happen to agree today:
//! a local block can never accept a value the global pipeline would reject, or
//! the other way round. The rules are unchanged from the previous inline
//! `validate_adjustments` checks — this is an extraction, not a relaxation.

use crate::{ColorGrading, HslAdjustments, PointColor, SidecarError};

fn invalid(message: impl Into<String>) -> Result<(), SidecarError> {
    Err(SidecarError::Invalid(message.into()))
}

/// The legacy grading `blending` default. `0.5` reproduces the pre-refinement
/// range edges exactly; it is exposed so a reset writes the same value the
/// deserializer default would instead of a hard-coded second magic number.
#[must_use]
pub fn default_color_grading_blending() -> f32 {
    0.5
}

/// Reads one optional band out of a whole [`HslAdjustments`] block.
pub type HslBandSelector = fn(&HslAdjustments) -> Option<crate::HslChannel>;

/// The eight HSL colour bands in their canonical order, paired with the field
/// selector of the persisted block. Kept as one list so a new band cannot be
/// added to the type without also being validated here.
pub const HSL_BANDS: [(&str, HslBandSelector); 8] = [
    ("red", |h| h.red),
    ("orange", |h| h.orange),
    ("yellow", |h| h.yellow),
    ("green", |h| h.green),
    ("cyan", |h| h.cyan),
    ("blue", |h| h.blue),
    ("violet", |h| h.violet),
    ("magenta", |h| h.magenta),
];

/// Resolve one HSL band by its stable local/CLI name.
///
/// A typo is a loud `None`, never a silent fall-through to another band.
#[must_use]
pub fn hsl_band(adjustments: &HslAdjustments, name: &str) -> Option<crate::HslChannel> {
    HSL_BANDS
        .iter()
        .find(|(band, _)| *band == name)
        .and_then(|(_, select)| select(adjustments))
}

/// Mutable access to one HSL band, creating the slot when it is unset.
pub fn hsl_band_mut<'a>(
    adjustments: &'a mut HslAdjustments,
    name: &str,
) -> Option<&'a mut crate::HslChannel> {
    let slot = match name {
        "red" => &mut adjustments.red,
        "orange" => &mut adjustments.orange,
        "yellow" => &mut adjustments.yellow,
        "green" => &mut adjustments.green,
        "cyan" => &mut adjustments.cyan,
        "blue" => &mut adjustments.blue,
        "violet" => &mut adjustments.violet,
        "magenta" => &mut adjustments.magenta,
        _ => return None,
    };
    Some(slot.get_or_insert_with(crate::HslChannel::default))
}

/// Mutable access to one HSL band slot without creating it.
pub fn hsl_band_slot_mut<'a>(
    adjustments: &'a mut HslAdjustments,
    name: &str,
) -> Option<&'a mut Option<crate::HslChannel>> {
    match name {
        "red" => Some(&mut adjustments.red),
        "orange" => Some(&mut adjustments.orange),
        "yellow" => Some(&mut adjustments.yellow),
        "green" => Some(&mut adjustments.green),
        "cyan" => Some(&mut adjustments.cyan),
        "blue" => Some(&mut adjustments.blue),
        "violet" => Some(&mut adjustments.violet),
        "magenta" => Some(&mut adjustments.magenta),
        _ => None,
    }
}

/// Validate a whole [`HslAdjustments`] block: block version plus the three
/// `-1..=1` shift fields of every present band.
pub fn validate_hsl(adjustments: &HslAdjustments) -> Result<(), SidecarError> {
    if adjustments.version != 1 {
        return invalid("unsupported hsl version");
    }
    for (name, select) in HSL_BANDS {
        let Some(channel) = select(adjustments) else {
            continue;
        };
        for (field, value) in [
            ("hue", channel.hue),
            ("saturation", channel.saturation),
            ("luminance", channel.luminance),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return invalid(format!("invalid hsl {name}.{field}"));
            }
        }
    }
    Ok(())
}

/// True when no stored band can change a pixel: every present band has all
/// three shifts at zero. An empty block and an all-zero block are the same
/// thing, which is what lets a renderer drop a neutral block.
#[must_use]
pub fn hsl_is_neutral(adjustments: &HslAdjustments) -> bool {
    HSL_BANDS.iter().all(|(_, select)| {
        select(adjustments).is_none_or(|channel| {
            channel.hue == 0.0 && channel.saturation == 0.0 && channel.luminance == 0.0
        })
    })
}

/// Validate a whole [`ColorGrading`] block: block version, `balance`
/// (`-1..=1`), `blending` (`0..=1`) and the three ranges.
pub fn validate_color_grading(grading: &ColorGrading) -> Result<(), SidecarError> {
    if grading.version != 1 {
        return invalid("unsupported color_grading version");
    }
    if !grading.balance.is_finite() || !(-1.0..=1.0).contains(&grading.balance) {
        return invalid("invalid color_grading balance");
    }
    if !grading.blending.is_finite() || !(0.0..=1.0).contains(&grading.blending) {
        return invalid("invalid color_grading blending");
    }
    for (name, range) in color_grading_ranges(grading) {
        if !range.hue_degrees.is_finite() || !(0.0..=360.0).contains(&range.hue_degrees) {
            return invalid(format!("invalid color_grading {name}.hue_degrees"));
        }
        if !range.saturation.is_finite() || !(0.0..=1.0).contains(&range.saturation) {
            return invalid(format!("invalid color_grading {name}.saturation"));
        }
        if !range.luminance.is_finite() || !(-1.0..=1.0).contains(&range.luminance) {
            return invalid(format!("invalid color_grading {name}.luminance"));
        }
    }
    Ok(())
}

/// The three grading ranges in canonical (shadows, midtones, highlights)
/// order, paired with their stable names.
pub fn color_grading_ranges(
    grading: &ColorGrading,
) -> [(&'static str, crate::ColorGradingRange); 3] {
    [
        ("shadows", grading.shadows),
        ("midtones", grading.midtones),
        ("highlights", grading.highlights),
    ]
}

/// Mutable access to one grading range by its stable name.
pub fn color_grading_range_mut<'a>(
    grading: &'a mut ColorGrading,
    name: &str,
) -> Option<&'a mut crate::ColorGradingRange> {
    match name {
        "shadows" => Some(&mut grading.shadows),
        "midtones" => Some(&mut grading.midtones),
        "highlights" => Some(&mut grading.highlights),
        _ => None,
    }
}

/// True when the whole block is pixel-neutral: no range tint, no range
/// luminance shift, and a centred balance. `blending` only moves the *edges*
/// of the three weights, so a neutral block with any blending is still neutral
/// — the kernel multiplies by `weight * saturation` and skips `saturation == 0`.
#[must_use]
pub fn color_grading_is_neutral(grading: &ColorGrading) -> bool {
    grading.balance == 0.0
        && color_grading_ranges(grading)
            .iter()
            .all(|(_, range)| range.saturation == 0.0 && range.luminance == 0.0)
}

/// True when the block carries nothing a user could have chosen *or* an effect
/// it could apply — including a range `hue_degrees`, which is a selection
/// parameter that is pixel-neutral on its own but must survive the next
/// saturation edit. This is the *storage* predicate (may the block be dropped
/// as "never edited"?), not the *pixel* predicate above.
#[must_use]
pub fn color_grading_is_unset(grading: &ColorGrading) -> bool {
    grading.balance == 0.0
        && grading.blending == default_color_grading_blending()
        && color_grading_ranges(grading)
            .iter()
            .all(|(_, range)| *range == crate::ColorGradingRange::neutral())
}

/// Maximum number of point-colour entries in one recipe, shared by the global
/// and the mask-local block.
pub const MAX_POINT_COLOR_ENTRIES: usize = 8;

/// Validate a whole [`PointColor`] block: block version, entry count, unique
/// non-empty ids and the five per-entry ranges.
pub fn validate_point_color(point_color: &PointColor) -> Result<(), SidecarError> {
    if point_color.version != 1 {
        return invalid("unsupported point_color version");
    }
    if point_color.entries.len() > MAX_POINT_COLOR_ENTRIES {
        return invalid("too many point_color entries (max 8)");
    }
    let mut seen = std::collections::HashSet::new();
    for entry in &point_color.entries {
        if entry.id.is_empty() || !seen.insert(entry.id.clone()) {
            return invalid("invalid point_color entry id (empty or duplicate)");
        }
        if !entry.hue_center.is_finite() || !(0.0..=360.0).contains(&entry.hue_center) {
            return invalid(format!("invalid point_color {} hue_center", entry.id));
        }
        if !entry.hue_range.is_finite() || !(0.0..=180.0).contains(&entry.hue_range) {
            return invalid(format!("invalid point_color {} hue_range", entry.id));
        }
        for (field, value) in [
            ("hue_shift", entry.hue_shift),
            ("saturation_shift", entry.saturation_shift),
            ("luminance_shift", entry.luminance_shift),
        ] {
            if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
                return invalid(format!("invalid point_color {} {field}", entry.id));
            }
        }
    }
    Ok(())
}

/// True when no entry can change a pixel: no entries at all, or every entry
/// has all three shifts at zero (the selection weights nothing).
#[must_use]
pub fn point_color_is_neutral(point_color: &PointColor) -> bool {
    point_color.entries.iter().all(|entry| {
        entry.hue_shift == 0.0 && entry.saturation_shift == 0.0 && entry.luminance_shift == 0.0
    })
}
