//! MASK-LOCAL-P1.2d accessors for the local detail block.
//!
//! These live beside (not inside) the schema type so the typed object stays a
//! pure data/validation contract. Every mutation validates against the shared
//! **global** detail rules *before* it stores anything, so a rejected value
//! leaves the layer byte-for-byte unchanged, and a sub-block that returns to its
//! neutral value removes itself, so a reset is byte-identical to a layer that was
//! never edited.
//!
//! Both sub-blocks are deliberately the *global* `lumina_sidecar::Sharpening` and
//! `lumina_sidecar::NoiseReduction`: a local sharpening/noise reduction is the
//! global stage at a different place in the chain, so a second local type would
//! be a dialect that could drift.
//!
//! Reset works on three levels — one sub-block (`sharpening`, `noise_reduction`),
//! the whole block (`detail`), and, implicitly, writing a sub-block back to its
//! neutral value, which drops just that sub-block.

use super::LocalAdjustments;
use crate::detail_block::{
    noise_reduction_field_range, noise_reduction_is_neutral, sharpening_field_range,
    sharpening_is_neutral,
};
use crate::{Detail, NoiseReduction, Sharpening};

/// The F-095/F-096 identity sharpening: `amount = 0`. The remaining fields carry
/// the global stage's own documented defaults, because a neutral block is only
/// ever a starting point for an edit, never a rendered state.
#[must_use]
pub fn neutral_local_sharpening() -> Sharpening {
    Sharpening {
        version: crate::detail_block::DETAIL_BLOCK_VERSION,
        amount: 0.0,
        radius: 1.0,
        detail: 0.5,
        masking: 0.0,
    }
}

/// A neutral, fully valid noise-reduction block (both strengths at zero).
#[must_use]
pub fn neutral_local_noise_reduction() -> NoiseReduction {
    NoiseReduction {
        version: crate::detail_block::DETAIL_BLOCK_VERSION,
        luminance: 0.0,
        color: 0.0,
    }
}

/// A neutral, fully valid detail block: both sub-blocks at their neutral state.
#[must_use]
pub fn neutral_local_detail() -> Detail {
    Detail {
        sharpening: Some(neutral_local_sharpening()),
        noise_reduction: Some(neutral_local_noise_reduction()),
    }
}

impl LocalAdjustments {
    /// True when the layer stores a detail value that can change a pixel.
    ///
    /// This is the routing predicate *and* the kernel-path predicate: an absent
    /// block and a persisted block whose sub-blocks are all neutral both read
    /// `false`, so a neutral block keeps every previously pinned P0/P1.1/P1.2a/
    /// P1.2b/P1.2c byte exactly. That is why the check is on the *content*, not
    /// on `self.detail.is_some()`.
    #[must_use]
    pub fn has_local_detail(&self) -> bool {
        self.detail
            .as_ref()
            .is_some_and(|detail| !detail.is_neutral())
    }

    /// True when the local sharpening sub-block can change a pixel. Split out so
    /// the routing rules and the CLI/GUI status can name the two detail stages
    /// individually.
    #[must_use]
    pub fn has_local_sharpening(&self) -> bool {
        self.detail
            .as_ref()
            .and_then(|detail| detail.sharpening.as_ref())
            .is_some_and(|sharpening| !sharpening_is_neutral(sharpening))
    }

    /// True when the local noise-reduction sub-block can change a pixel.
    #[must_use]
    pub fn has_local_noise_reduction(&self) -> bool {
        self.detail
            .as_ref()
            .and_then(|detail| detail.noise_reduction.as_ref())
            .is_some_and(|noise| !noise_reduction_is_neutral(noise))
    }

    /// Canonical status summary of the local detail block: `none` when the block
    /// cannot change a pixel, otherwise the non-neutral sub-block names joined
    /// with `+` in the canonical `sharpening`/`noise_reduction` order.
    ///
    /// The summary deliberately reports the **sub-block**, not the individual
    /// fields, because a sharpening block's neutrality is a property of its
    /// `amount` alone — the flat-area `masking` is a multiplier, not a gate.
    #[must_use]
    pub fn detail_summary(&self) -> String {
        let Some(detail) = &self.detail else {
            return "none".into();
        };
        if detail.is_neutral() {
            return "none".into();
        }
        let mut stored: Vec<&str> = Vec::new();
        if self.has_local_sharpening() {
            stored.push("sharpening");
        }
        if self.has_local_noise_reduction() {
            stored.push("noise_reduction");
        }
        stored.join("+")
    }

    /// Set one local sharpening field, creating the sub-block on demand.
    ///
    /// The field name is checked *before* the sub-block is created, so a typo is
    /// loud even on a layer that was never edited, and the value is validated
    /// with the shared **global** F-095/F-096 range before it is stored. Writing
    /// the amount back to zero drops the whole sub-block, so the layer is then
    /// byte-identical to one that was never edited.
    pub fn set_local_sharpening_field(&mut self, field: &str, value: f64) -> Result<(), String> {
        let Some((low, high)) = sharpening_field_range(field) else {
            return Err(format!("unknown local sharpening field `{field}`"));
        };
        if !value.is_finite() || !(low..=high).contains(&value) {
            return Err(format!(
                "local sharpening {field} must be finite and in {low}..={high}, got {value}"
            ));
        }
        let mut sharpening = self.local_sharpening_or_neutral();
        match field {
            "amount" => sharpening.amount = value as f32,
            "radius" => sharpening.radius = value as f32,
            "detail" => sharpening.detail = value as f32,
            "masking" => sharpening.masking = value as f32,
            _ => unreachable!("looked-up local sharpening field"),
        }
        crate::validate_sharpening(&sharpening)
            .map_err(|error| format!("local sharpening: {error}"))?;
        self.store_local_detail(Detail {
            sharpening: Some(sharpening),
            noise_reduction: self.detail.and_then(|detail| detail.noise_reduction),
        });
        Ok(())
    }

    /// Set one local noise-reduction field, creating the sub-block on demand.
    /// Same loudness and shared-global-range contract as
    /// [`Self::set_local_sharpening_field`].
    pub fn set_local_noise_reduction_field(
        &mut self,
        field: &str,
        value: f64,
    ) -> Result<(), String> {
        let Some((low, high)) = noise_reduction_field_range(field) else {
            return Err(format!("unknown local noise reduction field `{field}`"));
        };
        if !value.is_finite() || !(low..=high).contains(&value) {
            return Err(format!(
                "local noise reduction {field} must be finite and in {low}..={high}, got {value}"
            ));
        }
        let mut noise = self.local_noise_reduction_or_neutral();
        match field {
            "luminance" => noise.luminance = value as f32,
            "color" => noise.color = value as f32,
            _ => unreachable!("looked-up local noise reduction field"),
        }
        crate::validate_noise_reduction(&noise)
            .map_err(|error| format!("local noise reduction: {error}"))?;
        self.store_local_detail(Detail {
            sharpening: self.detail.and_then(|detail| detail.sharpening),
            noise_reduction: Some(noise),
        });
        Ok(())
    }

    /// Reset one sub-block. `sharpening` and `noise_reduction` are the only
    /// accepted names; anything else (including the whole-block `detail`) is a
    /// loud error here, because this function is the *per-area* reset. Use
    /// [`Self::reset_local_detail`] for the whole block.
    pub fn reset_local_detail_field(&mut self, field: &str) -> Result<(), String> {
        match field {
            "sharpening" => {
                let noise_reduction = self.detail.and_then(|detail| detail.noise_reduction);
                self.store_local_detail(Detail {
                    sharpening: None,
                    noise_reduction,
                });
                Ok(())
            }
            "noise_reduction" => {
                let sharpening = self.detail.and_then(|detail| detail.sharpening);
                self.store_local_detail(Detail {
                    sharpening,
                    noise_reduction: None,
                });
                Ok(())
            }
            other => Err(format!(
                "unknown local detail field `{other}`; reset `sharpening` or `noise_reduction`"
            )),
        }
    }

    /// Reset the whole local detail block. The block disappears entirely, so a
    /// reset is byte-identical to a layer that was never edited.
    pub fn reset_local_detail(&mut self) {
        self.detail = None;
    }

    fn local_sharpening_or_neutral(&self) -> Sharpening {
        self.detail
            .and_then(|detail| detail.sharpening)
            .unwrap_or_else(neutral_local_sharpening)
    }

    fn local_noise_reduction_or_neutral(&self) -> NoiseReduction {
        self.detail
            .and_then(|detail| detail.noise_reduction)
            .unwrap_or_else(neutral_local_noise_reduction)
    }

    /// Commit one already validated sub-block set, dropping a sub-block that is
    /// neutral and then dropping the whole container once both are gone.
    ///
    /// A sharpening is *not* neutral just because `masking` is zero: masking is
    /// the flat-area suppression multiplier, so `masking = 0` is the strongest
    /// setting, not a no-op. Only `amount == 0` removes the sub-block.
    fn store_local_detail(&mut self, detail: Detail) {
        let sharpening = detail
            .sharpening
            .filter(|sharpening| !sharpening_is_neutral(sharpening));
        let noise_reduction = detail
            .noise_reduction
            .filter(|noise| !noise_reduction_is_neutral(noise));
        self.detail = match (sharpening, noise_reduction) {
            (None, None) => None,
            (sharpening, noise_reduction) => Some(Detail {
                sharpening,
                noise_reduction,
            }),
        };
    }
}
