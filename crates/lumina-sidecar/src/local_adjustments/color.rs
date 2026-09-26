//! MASK-LOCAL-P1.2b accessors for the local per-pixel color block.
//!
//! These live beside (not inside) the schema type so the typed object stays a
//! pure data/validation contract. Every mutation validates against the shared
//! global color rules *before* it stores anything, so a rejected value leaves
//! the layer byte-for-byte unchanged. The rules themselves are the very same
//! `validate_hsl`/`validate_point_color`/`validate_color_grading` functions the
//! global recipe uses — a local colour block can never accept a value the
//! global pipeline would reject.

use super::LocalAdjustments;
use crate::{
    color_grading_is_neutral, color_grading_is_unset, hsl_band, hsl_band_mut, hsl_band_slot_mut,
    hsl_is_neutral, point_color_is_neutral, HslAdjustments, HslChannel, PointColorEntry,
};

/// The three HSL shift fields, in their canonical display/serialization order.
pub const LOCAL_HSL_FIELDS: [&str; 3] = ["hue", "saturation", "luminance"];

/// The three color-grading ranges in their canonical order.
pub const LOCAL_GRADING_RANGES: [&str; 3] = ["shadows", "midtones", "highlights"];

/// The point-colour entry fields, in the order the CLI value list uses.
pub const LOCAL_POINT_COLOR_FIELDS: [&str; 5] = [
    "hue_center",
    "hue_range",
    "hue_shift",
    "saturation_shift",
    "luminance_shift",
];

/// A neutral, fully valid HSL block (no band stored).
#[must_use]
pub fn neutral_local_hsl() -> HslAdjustments {
    HslAdjustments {
        version: 1,
        ..Default::default()
    }
}

/// One point-colour entry built from the five validated fields.
#[must_use]
pub fn local_point_color_entry(
    id: String,
    hue_center: f32,
    hue_range: f32,
    hue_shift: f32,
    saturation_shift: f32,
    luminance_shift: f32,
) -> PointColorEntry {
    PointColorEntry {
        id,
        hue_center,
        hue_range,
        hue_shift,
        saturation_shift,
        luminance_shift,
    }
}

impl LocalAdjustments {
    /// True when the layer stores a color control that can change a pixel.
    ///
    /// This is the routing predicate *and* the kernel-path predicate: a layer
    /// that only carries a neutral or absent color block must keep every
    /// stand-in route on the CPU reference path exactly as before, so it
    /// reports `false` here. The two additive scalars are part of the same
    /// block, so a vibrance-only or saturation-only layer counts too.
    #[must_use]
    pub fn has_local_color(&self) -> bool {
        self.vibrance != 0.0
            || self.saturation != 0.0
            || self.has_local_hsl()
            || self.has_local_point_color()
            || self.has_local_color_grading()
    }

    /// True when the stored HSL block has at least one non-neutral band.
    #[must_use]
    pub fn has_local_hsl(&self) -> bool {
        self.hsl.as_ref().is_some_and(|hsl| !hsl_is_neutral(hsl))
    }

    /// True when the stored point-colour block has at least one shifting entry.
    #[must_use]
    pub fn has_local_point_color(&self) -> bool {
        self.point_color
            .as_ref()
            .is_some_and(|point_color| !point_color_is_neutral(point_color))
    }

    /// True when the stored grading block can tint or lift a pixel.
    #[must_use]
    pub fn has_local_color_grading(&self) -> bool {
        self.color_grading
            .as_ref()
            .is_some_and(|grading| !color_grading_is_neutral(grading))
    }

    /// Compact, deterministic summary of the stored local color state used by
    /// the CLI status line: each block is `none` when neutral, otherwise a
    /// stable `<block>:<count>` form.
    #[must_use]
    pub fn hsl_summary(&self) -> String {
        let Some(hsl) = &self.hsl else {
            return "none".into();
        };
        let stored = crate::HSL_BANDS
            .iter()
            .filter(|(_, select)| {
                select(hsl).is_some_and(|channel| channel != HslChannel::default())
            })
            .count();
        if stored == 0 {
            "none".into()
        } else {
            format!("{stored}bands")
        }
    }

    /// `none` for an absent or neutral block, otherwise `<n>entries`.
    #[must_use]
    pub fn point_color_summary(&self) -> String {
        let Some(point_color) = &self.point_color else {
            return "none".into();
        };
        if point_color_is_neutral(point_color) {
            return "none".into();
        }
        format!("{}entries", point_color.entries.len())
    }

    /// `none` for an absent or neutral block, otherwise the stored ranges.
    #[must_use]
    pub fn color_grading_summary(&self) -> String {
        let Some(grading) = &self.color_grading else {
            return "none".into();
        };
        if color_grading_is_neutral(grading) {
            return "none".into();
        }
        let ranges: Vec<&str> = LOCAL_GRADING_RANGES
            .iter()
            .copied()
            .filter(|name| {
                crate::color_grading_ranges(grading)
                    .into_iter()
                    .any(|(range_name, range)| {
                        range_name == *name && (range.saturation != 0.0 || range.luminance != 0.0)
                    })
            })
            .collect();
        let mut parts = Vec::new();
        if grading.balance != 0.0 {
            parts.push("balance".to_string());
        }
        parts.extend(ranges.into_iter().map(str::to_string));
        if parts.is_empty() {
            "none".into()
        } else {
            parts.join("+")
        }
    }
    /// Read one local HSL band. A band that was never edited reads `None`,
    /// which every editor resolves to the neutral triple.
    #[must_use]
    pub fn local_hsl_band(&self, band: &str) -> Option<HslChannel> {
        self.hsl.as_ref().and_then(|hsl| hsl_band(hsl, band))
    }

    /// Set one HSL shift of one band, creating the band block on demand.
    ///
    /// The band and field names are checked *before* the block is created, so a
    /// typo is loud even on a layer that was never edited. The value is
    /// validated with the shared global `-1..=1` rule before it is stored, so a
    /// refused value leaves the layer byte-for-byte unchanged.
    pub fn set_local_hsl_band(
        &mut self,
        band: &str,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        if !LOCAL_HSL_FIELDS.contains(&field) {
            return Err(format!("unknown local hsl field `{field}`"));
        }
        if !crate::HSL_BANDS.iter().any(|(name, _)| *name == band) {
            return Err(format!("unknown local hsl band `{band}`"));
        }
        if !value.is_finite() || !(-1.0..=1.0).contains(&value) {
            return Err(format!(
                "local hsl {band}.{field} must be finite and in -1..=1, got {value}"
            ));
        }
        let value = value as f32;
        let hsl = self.hsl.get_or_insert_with(neutral_local_hsl);
        let channel =
            hsl_band_mut(hsl, band).ok_or_else(|| format!("unknown local hsl band `{band}`"))?;
        match field {
            "hue" => channel.hue = value,
            "saturation" => channel.saturation = value,
            "luminance" => channel.luminance = value,
            _ => unreachable!("validated local hsl field"),
        }
        // A band whose three shifts are all back at zero is byte-identical to a
        // band that was never edited, so the slot itself goes away.
        let emptied = *channel == HslChannel::default();
        if let Some(slot) = hsl_band_slot_mut(hsl, band) {
            if emptied {
                *slot = None;
            }
        }
        self.drop_neutral_color();
        Ok(())
    }

    /// Reset one HSL band. The band disappears when it is neutral again, and
    /// the whole block disappears when the last band is gone, so a reset is
    /// byte-identical to a layer that was never edited.
    pub fn reset_local_hsl_band(&mut self, band: &str) -> Result<(), String> {
        if !crate::HSL_BANDS.iter().any(|(name, _)| *name == band) {
            return Err(format!("unknown local hsl band `{band}`"));
        }
        if let Some(hsl) = &mut self.hsl {
            if let Some(slot) = hsl_band_slot_mut(hsl, band) {
                *slot = None;
            }
            if !crate::HSL_BANDS.iter().any(|(_, s)| s(hsl).is_some()) {
                self.hsl = None;
            }
        }
        Ok(())
    }

    /// Drop the whole local HSL block.
    pub fn reset_local_hsl(&mut self) {
        self.hsl = None;
    }
    /// Remove a block that carries nothing at all any more, so that a value
    /// typed back to its neutral is byte-identical to "never edited".
    ///
    /// This is deliberately the *storage* predicate, not the *pixel* one: a
    /// grading range whose only stored value is a `hue_degrees` selection is
    /// pixel-neutral (the kernel skips a zero-saturation range) but must still
    /// be kept, because the very next saturation edit is supposed to use that
    /// hue. An explicit `reset_local_*` always drops the block.
    pub(super) fn drop_neutral_color(&mut self) {
        if !self.hsl_has_band() {
            self.hsl = None;
        }
        if self
            .point_color
            .as_ref()
            .is_some_and(|point_color| point_color.entries.is_empty())
        {
            self.point_color = None;
        }
        if self
            .color_grading
            .as_ref()
            .is_some_and(color_grading_is_unset)
        {
            self.color_grading = None;
        }
    }

    /// True when the stored HSL block still holds at least one band slot.
    pub(super) fn hsl_has_band(&self) -> bool {
        self.hsl.as_ref().is_some_and(|hsl| {
            crate::HSL_BANDS
                .iter()
                .any(|(_, select)| select(hsl).is_some())
        })
    }
}
