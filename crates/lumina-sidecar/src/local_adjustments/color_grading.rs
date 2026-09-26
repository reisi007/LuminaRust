//! MASK-LOCAL-P1.2b accessors for the local Point Color and Color Grading
//! blocks, beside `color.rs` (the HSL half) for the same reason: the typed
//! object stays a pure data/validation contract and every mutation validates
//! against the shared global rules *before* it stores anything.

use super::color::{LOCAL_GRADING_RANGES, LOCAL_POINT_COLOR_FIELDS};
use super::local_point_color_entry;
use super::LocalAdjustments;
use crate::{
    color_grading_is_neutral, color_grading_range_mut, ColorGrading, ColorGradingRange, PointColor,
    PointColorEntry, MAX_POINT_COLOR_ENTRIES,
};

impl LocalAdjustments {
    /// Read the stored point-colour entries in persisted order.
    #[must_use]
    pub fn local_point_color_entries(&self) -> &[PointColorEntry] {
        self.point_color
            .as_ref()
            .map(|point_color| point_color.entries.as_slice())
            .unwrap_or(&[])
    }

    /// Append one point-colour entry with a fresh stable id and return it.
    ///
    /// The entry is validated with the shared global rules (including the
    /// 8-entry limit) *before* it is appended, so a refused entry leaves the
    /// block unchanged.
    pub fn add_local_point_color_entry(
        &mut self,
        hue_center: f64,
        hue_range: f64,
        hue_shift: f64,
        saturation_shift: f64,
        luminance_shift: f64,
    ) -> Result<String, String> {
        let values = [
            hue_center,
            hue_range,
            hue_shift,
            saturation_shift,
            luminance_shift,
        ];
        if values.iter().any(|value| !value.is_finite()) {
            return Err("local point color values must be finite".into());
        }
        if !(0.0..=360.0).contains(&hue_center) {
            return Err(format!(
                "local point color hue_center must be in 0..=360, got {hue_center}"
            ));
        }
        if !(0.0..=180.0).contains(&hue_range) {
            return Err(format!(
                "local point color hue_range must be in 0..=180, got {hue_range}"
            ));
        }
        if [hue_shift, saturation_shift, luminance_shift]
            .iter()
            .any(|value| !(-1.0..=1.0).contains(value))
        {
            return Err("local point color shifts must be in -1..=1".into());
        }
        let id = PointColorEntry::next_id(self.local_point_color_entries());
        let entry = local_point_color_entry(
            id.clone(),
            hue_center as f32,
            hue_range as f32,
            hue_shift as f32,
            saturation_shift as f32,
            luminance_shift as f32,
        );
        let point_color = self.point_color.get_or_insert_with(|| PointColor {
            version: 1,
            entries: Vec::new(),
        });
        if point_color.entries.len() >= MAX_POINT_COLOR_ENTRIES {
            return Err(format!(
                "local point color already has {MAX_POINT_COLOR_ENTRIES} entries (the maximum)"
            ));
        }
        point_color.entries.push(entry);
        Ok(id)
    }

    /// Set one shift of one existing point-colour entry.
    pub fn set_local_point_color_field(
        &mut self,
        id: &str,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        if !LOCAL_POINT_COLOR_FIELDS.contains(&field) {
            return Err(format!("unknown local point color field `{field}`"));
        }
        if !value.is_finite() {
            return Err(format!(
                "local point color {field} must be finite, got {value}"
            ));
        }
        let range = match field {
            "hue_center" => 0.0..=360.0,
            "hue_range" => 0.0..=180.0,
            _ => -1.0..=1.0,
        };
        if !range.contains(&value) {
            return Err(format!(
                "local point color {id}.{field} must be in {}..={}, got {value}",
                range.start(),
                range.end()
            ));
        }
        let value = value as f32;
        let Some(point_color) = &mut self.point_color else {
            return Err(format!("mask layer has no local point color entry `{id}`"));
        };
        let Some(entry) = point_color.entries.iter_mut().find(|entry| entry.id == id) else {
            return Err(format!("mask layer has no local point color entry `{id}`"));
        };
        match field {
            "hue_center" => entry.hue_center = value,
            "hue_range" => entry.hue_range = value,
            "hue_shift" => entry.hue_shift = value,
            "saturation_shift" => entry.saturation_shift = value,
            "luminance_shift" => entry.luminance_shift = value,
            _ => unreachable!("validated local point color field"),
        }
        self.drop_neutral_color();
        Ok(())
    }

    /// Remove one point-colour entry by its stable id. The whole block is
    /// dropped once the last entry is gone.
    pub fn remove_local_point_color_entry(&mut self, id: &str) -> Result<(), String> {
        let Some(point_color) = &mut self.point_color else {
            return Err(format!("mask layer has no local point color entry `{id}`"));
        };
        let before = point_color.entries.len();
        point_color.entries.retain(|entry| entry.id != id);
        if point_color.entries.len() == before {
            return Err(format!("mask layer has no local point color entry `{id}`"));
        }
        if point_color.entries.is_empty() {
            self.point_color = None;
        }
        self.drop_neutral_color();
        Ok(())
    }

    /// Drop the whole local point-colour block.
    pub fn reset_local_point_color(&mut self) {
        self.point_color = None;
    }

    /// Read one local grading range. A block that was never edited reads
    /// `None`, which every editor resolves to the neutral range.
    #[must_use]
    pub fn local_color_grading_range(&self, range: &str) -> Option<ColorGradingRange> {
        let grading = self.color_grading.as_ref()?;
        LOCAL_GRADING_RANGES
            .iter()
            .position(|name| *name == range)
            .map(|index| [grading.shadows, grading.midtones, grading.highlights][index])
    }

    /// Read the stored local grading block, or `None` when there is none.
    #[must_use]
    pub fn local_color_grading(&self) -> Option<ColorGrading> {
        self.color_grading.clone()
    }

    /// Set one field of the local grading block.
    ///
    /// `target` is `shadows`/`midtones`/`highlights`/`balance`/`blending`; the
    /// value is validated with the shared global rule for that field before it
    /// is stored, so a refused value leaves the layer byte-for-byte unchanged.
    pub fn set_local_color_grading_field(
        &mut self,
        target: &str,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        if !LOCAL_GRADING_RANGES.contains(&target) && !matches!(target, "balance" | "blending") {
            return Err(format!("unknown local color grading target `{target}`"));
        }
        if !value.is_finite() {
            return Err(format!(
                "local color grading {target} must be finite, got {value}"
            ));
        }
        let value = value as f32;
        if let Some(range) = LOCAL_GRADING_RANGES.iter().find(|name| **name == target) {
            let field = match field {
                "hue" | "hue_degrees" => "hue_degrees",
                "saturation" => "saturation",
                "luminance" => "luminance",
                _ => {
                    return Err(format!("unknown local color grading field `{field}`"));
                }
            };
            let range_value = match field {
                "hue_degrees" => 0.0..=360.0,
                "saturation" => 0.0..=1.0,
                _ => -1.0..=1.0,
            };
            if !range_value.contains(&value) {
                return Err(format!(
                    "local color grading {target}.{field} must be in {}..={}, got {value}",
                    range_value.start(),
                    range_value.end()
                ));
            }
            let grading = self.color_grading.get_or_insert_with(ColorGrading::neutral);
            let slot = color_grading_range_mut(grading, range)
                .ok_or_else(|| format!("unknown local color grading target `{target}`"))?;
            match field {
                "hue_degrees" => slot.hue_degrees = value,
                "saturation" => slot.saturation = value,
                "luminance" => slot.luminance = value,
                _ => unreachable!("validated local color grading field"),
            }
        } else {
            // `balance`/`blending` are the two scalar fields of the block; a
            // mismatching `field` is a typo and stays loud.
            let field = match field {
                "value" => target,
                other if other == target => target,
                _ => return Err(format!("unknown local color grading field `{field}`")),
            };
            let allowed = match field {
                "balance" => -1.0..=1.0,
                _ => 0.0..=1.0,
            };
            if !allowed.contains(&value) {
                return Err(format!(
                    "local color grading {field} must be in {}..={}, got {value}",
                    allowed.start(),
                    allowed.end()
                ));
            }
            let grading = self.color_grading.get_or_insert_with(ColorGrading::neutral);
            match field {
                "balance" => grading.balance = value,
                _ => grading.blending = value,
            }
        }
        self.drop_neutral_color();
        Ok(())
    }

    /// Reset one grading range (or `balance`/`blending`) to its neutral value.
    /// The whole block is dropped once it is neutral again.
    pub fn reset_local_color_grading_field(&mut self, target: &str) -> Result<(), String> {
        if target == "color_grading" || target == "color" {
            self.reset_local_color_grading();
            return Ok(());
        }
        if LOCAL_GRADING_RANGES.contains(&target) {
            let Some(grading) = &mut self.color_grading else {
                return Ok(());
            };
            let slot = color_grading_range_mut(grading, target)
                .ok_or_else(|| format!("unknown local color grading target `{target}`"))?;
            *slot = ColorGradingRange::neutral();
        } else if matches!(target, "balance" | "blending") {
            let Some(grading) = &mut self.color_grading else {
                return Ok(());
            };
            if target == "balance" {
                grading.balance = 0.0;
            } else {
                grading.blending = crate::default_color_grading_blending();
            }
        } else {
            return Err(format!("unknown local color grading target `{target}`"));
        }
        if color_grading_is_neutral(self.color_grading.as_ref().expect("block is present")) {
            self.color_grading = None;
        }
        Ok(())
    }

    /// Drop the whole local grading block.
    pub fn reset_local_color_grading(&mut self) {
        self.color_grading = None;
    }

    /// Drop every local color block: HSL, point colour, grading, vibrance and
    /// saturation. A no-op on a layer that carries no colour.
    pub fn reset_local_color(&mut self) {
        self.reset_local_hsl();
        self.reset_local_point_color();
        self.reset_local_color_grading();
        self.vibrance = 0.0;
        self.saturation = 0.0;
    }
}
